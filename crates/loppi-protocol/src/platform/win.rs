use super::*;
use std::{
    ffi::OsStr,
    fs::File,
    mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Diagnostics::ToolHelp::*, RemoteDesktop::*, SystemInformation::*, Threading::*},
};

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn token_user() -> io::Result<Vec<u8>> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut size = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
        let mut bytes = vec![0u8; size as usize];
        let ok = GetTokenInformation(token, TokenUser, bytes.as_mut_ptr().cast(), size, &mut size);
        CloseHandle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*(bytes.as_ptr().cast::<TOKEN_USER>());
        let size = GetLengthSid(user.User.Sid);
        let mut sid = vec![0u8; size as usize];
        if CopySid(size, sid.as_mut_ptr().cast(), user.User.Sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(sid)
    }
}

pub fn session_id(pid: u32) -> io::Result<u32> {
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut session) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(session)
}

pub fn verify_process(pid: u32) -> io::Result<()> {
    unsafe {
        if session_id(pid)? != session_id(GetCurrentProcessId())? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC session mismatch",
            ));
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut token = ptr::null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        CloseHandle(process);
        if opened == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut size = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
        let mut bytes = vec![0u8; size as usize];
        let ok = GetTokenInformation(token, TokenUser, bytes.as_mut_ptr().cast(), size, &mut size);
        CloseHandle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*(bytes.as_ptr().cast::<TOKEN_USER>());
        let own = token_user()?;
        if EqualSid(own.as_ptr() as _, user.User.Sid) == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC user mismatch",
            ));
        }
        Ok(())
    }
}

/// Owner-only, protected DACL. Keep the allocation alive until the OS call returns.
pub struct Security(pub PSECURITY_DESCRIPTOR);
impl Security {
    pub fn private() -> io::Result<Self> {
        Self::from_sid(&token_user()?)
    }
    pub fn session() -> io::Result<Self> {
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut size = 0;
            GetTokenInformation(token, TokenLogonSid, ptr::null_mut(), 0, &mut size);
            let mut bytes = vec![0u8; size as usize];
            let ok = GetTokenInformation(
                token,
                TokenLogonSid,
                bytes.as_mut_ptr().cast(),
                size,
                &mut size,
            );
            CloseHandle(token);
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let groups = &*(bytes.as_ptr() as *const TOKEN_GROUPS);
            if groups.GroupCount != 1 {
                return Err(io::Error::other("login SID unavailable"));
            }
            let sid = groups.Groups[0].Sid;
            Self::from_sid(std::slice::from_raw_parts(
                sid.cast(),
                GetLengthSid(sid) as usize,
            ))
        }
    }
    fn from_sid(sid: &[u8]) -> io::Result<Self> {
        unsafe {
            let mut text = ptr::null_mut();
            if ConvertSidToStringSidW(sid.as_ptr() as _, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            let sddl = wide(format!("D:P(A;OICI;FA;;;{sid_text})"));
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(descriptor))
        }
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub fn set_mode(path: impl AsRef<Path>, _mode: u32) -> io::Result<()> {
    // POSIX numeric bits are not Windows permissions: all private paths use an explicit owner DACL.
    let security = Security::private()?;
    let path = wide(path.as_ref());
    let mut present = 0;
    let mut defaulted = 0;
    let mut acl = ptr::null_mut();
    unsafe {
        if GetSecurityDescriptorDacl(security.0, &mut present, &mut acl, &mut defaulted) == 0 {
            return Err(io::Error::last_os_error());
        }
        let error = SetNamedSecurityInfoW(
            path.as_ptr() as _,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        );
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }
    Ok(())
}

/// Minimal private-file builder. Windows CreateFile receives the DACL at creation,
/// rather than exposing a file between creation and a later chmod equivalent.
#[derive(Default)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    create: bool,
    new: bool,
    truncate: bool,
    flags: u32,
}
impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn read(&mut self, v: bool) -> &mut Self {
        self.read = v;
        self
    }
    pub fn write(&mut self, v: bool) -> &mut Self {
        self.write = v;
        self
    }
    pub fn append(&mut self, v: bool) -> &mut Self {
        self.append = v;
        self
    }
    pub fn create(&mut self, v: bool) -> &mut Self {
        self.create = v;
        self
    }
    pub fn create_new(&mut self, v: bool) -> &mut Self {
        self.new = v;
        self
    }
    pub fn truncate(&mut self, v: bool) -> &mut Self {
        self.truncate = v;
        self
    }
    pub fn mode(&mut self, _mode: u32) -> &mut Self {
        self
    }
    pub fn custom_flags(&mut self, flags: u32) -> &mut Self {
        self.flags = flags;
        self
    }
    pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let security = Security::private()?;
        let attributes = security.attributes();
        let path = wide(path.as_ref());
        let access = (if self.read { GENERIC_READ } else { 0 })
            | (if self.write { GENERIC_WRITE } else { 0 })
            | (if self.append { FILE_APPEND_DATA } else { 0 });
        let disposition = if self.new {
            CREATE_NEW
        } else if self.create && self.truncate {
            CREATE_ALWAYS
        } else if self.create {
            OPEN_ALWAYS
        } else if self.truncate {
            TRUNCATE_EXISTING
        } else {
            OPEN_EXISTING
        };
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                &attributes,
                disposition,
                FILE_FLAG_OPEN_REPARSE_POINT | self.flags,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_handle(handle) };
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reparse point is not a private regular file",
            ));
        }
        Ok(file)
    }
}
pub const NOFOLLOW: u32 = FILE_FLAG_OPEN_REPARSE_POINT;

pub fn open_directory(path: impl AsRef<Path>) -> io::Result<File> {
    use std::os::windows::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path.as_ref())?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "journal root is not a directory",
        ));
    }
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "journal root is a reparse point",
        ));
    }
    // Lock a regular file: LockFileEx does not lock directory handles.
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path.as_ref().join(".publication.lock"))
}
pub fn sync_directory(path: impl AsRef<Path>) -> io::Result<()> {
    // Windows rename/publication uses write-through operations; validate that the volume is still present.
    if !std::fs::metadata(path)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "directory missing",
        ));
    }
    Ok(())
}
pub fn parent_pid() -> i32 {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return -1;
        }
        let mut entry: PROCESSENTRY32W = mem::zeroed();
        entry.dwSize = mem::size_of_val(&entry) as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        let mut parent = -1;
        while ok != 0 {
            if entry.th32ProcessID == GetCurrentProcessId() {
                parent = entry.th32ParentProcessID as i32;
                break;
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        parent
    }
}
pub fn parent_alive(pid: i32) -> bool {
    if parent_pid() != pid {
        return false;
    }
    unsafe {
        let p = OpenProcess(SYNCHRONIZE, 0, pid as u32);
        if p.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(p, 0) == WAIT_TIMEOUT;
        CloseHandle(p);
        alive
    }
}
pub fn hard_exit(code: i32) -> ! {
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    std::process::abort()
}
pub fn watch_process_exit(pid: u32) -> io::Result<()> {
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = handle as usize;
    std::thread::spawn(move || {
        unsafe {
            WaitForSingleObject(handle as HANDLE, INFINITE);
            CloseHandle(handle as HANDLE);
        }
        hard_exit(70)
    });
    Ok(())
}
pub fn login_id() -> io::Result<i64> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut statistics: TOKEN_STATISTICS = mem::zeroed();
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenStatistics,
            (&mut statistics as *mut TOKEN_STATISTICS).cast(),
            mem::size_of_val(&statistics) as u32,
            &mut length,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(((statistics.AuthenticationId.HighPart as i64) << 32)
            | statistics.AuthenticationId.LowPart as i64)
    }
}
pub fn monotonic_ns() -> io::Result<i64> {
    use windows_sys::Win32::System::Performance::*;
    let (mut count, mut frequency) = (0, 0);
    if unsafe { QueryPerformanceCounter(&mut count) } == 0
        || unsafe { QueryPerformanceFrequency(&mut frequency) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    i64::try_from((count as i128) * 1_000_000_000 / (frequency as i128)).map_err(io::Error::other)
}
pub fn require_supported() -> io::Result<()> {
    // RtlGetVersion is unaffected by compatibility-version manifests.
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut OSVERSIONINFOW) -> i32;
    }
    let mut version: OSVERSIONINFOW = unsafe { mem::zeroed() };
    version.dwOSVersionInfoSize = mem::size_of_val(&version) as u32;
    if unsafe { RtlGetVersion(&mut version) } != 0
        || version.dwBuildNumber < 22000
        || !cfg!(target_arch = "x86_64")
    {
        return Err(io::Error::other(
            "loppi requires Windows 11 x64 (build 22000 or later)",
        ));
    }
    Ok(())
}

pub fn initialize_process() -> io::Result<()> {
    require_supported()?;
    use windows_sys::Win32::System::Console::*;
    // Windows children can otherwise retain the parent's extra inheritable
    // stdout/stderr handles even when their own stdio is redirected to NUL.
    // Rust duplicates explicitly requested stdio when launching each child.
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let handle = unsafe { GetStdHandle(which) };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

pub fn confirm_cleanup() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let message = wide(
        "将永久清除选中范围的录音、文字、笔记、截图和相关声纹样例。已经导出或发送给服务商的副本无法从这里撤回。",
    );
    let title = wide("清除全部数据");
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDOK
    }
}

pub fn volume_id(path: impl AsRef<Path>) -> io::Result<u64> {
    let path = wide(path.as_ref());
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.dwVolumeSerialNumber as u64)
}
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let from = wide(from.as_ref());
    let to = wide(to.as_ref());
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide(from);
    let to = wide(to);
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn sync_file(path: impl AsRef<Path>) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()
}

fn process_image(pid: u32) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut bytes = vec![0u16; 32768];
    let mut size = bytes.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, bytes.as_mut_ptr(), &mut size) };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    bytes.truncate(size as usize);
    std::fs::canonicalize(std::ffi::OsString::from_wide(&bytes))
}

/// Ask only this installation's same-user/session GUI to take its normal exit
/// route. Never kills a process or accepts a request-supplied process identity.
pub fn stop_installed_gui(directory: &Path) -> io::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    struct Search {
        image: std::path::PathBuf,
        windows: Vec<(u32, HWND)>,
    }
    unsafe extern "system" fn each(hwnd: HWND, param: LPARAM) -> i32 {
        let search = unsafe { &mut *(param as *mut Search) };
        if unsafe { GetPropW(hwnd, wide("LoppiEntry").as_ptr()) }.is_null() {
            return 1;
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if verify_process(pid).is_ok() && process_image(pid).ok().as_ref() == Some(&search.image) {
            search.windows.push((pid, hwnd));
        }
        1
    }
    let image = std::fs::canonicalize(directory.join("loppi-desktop.exe"))?;
    let mut search = Search {
        image,
        windows: Vec::new(),
    };
    if unsafe { EnumWindows(Some(each), (&mut search as *mut Search) as isize) } == 0 {
        return Err(io::Error::last_os_error());
    }
    for (pid, hwnd) in search.windows {
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            continue;
        }
        let posted = unsafe { PostMessageW(hwnd, WM_APP + 43, 0, 0) };
        let stopped = if posted != 0 {
            (unsafe { WaitForSingleObject(handle, 15000) }) == WAIT_OBJECT_0
        } else {
            false
        };
        unsafe {
            CloseHandle(handle);
        }
        if !stopped {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "loppi GUI did not complete normal shutdown; installation was not changed",
            ));
        }
    }
    Ok(())
}

pub fn wait_installed_helpers(directory: &Path) -> io::Result<()> {
    let wanted = ["loppi-core.exe", "loppi-worker.exe"]
        .map(|name| std::fs::canonicalize(directory.join(name)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
        entry.dwSize = mem::size_of_val(&entry) as u32;
        let mut busy = false;
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
        while ok != 0 {
            let name = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_lowercase();
            if matches!(name.as_str(), "loppi-core.exe" | "loppi-worker.exe")
                && verify_process(entry.th32ProcessID).is_ok()
            {
                if let Ok(image) = process_image(entry.th32ProcessID) {
                    busy |= wanted
                        .iter()
                        .any(|path| path.as_ref().is_ok_and(|path| path == &image));
                }
            }
            ok = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe {
            CloseHandle(snapshot);
        }
        if !busy {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "loppi helpers still hold installation files; installation was not changed",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Third-party helpers cannot install our parent watchdog. A private job owns
/// their lifetime instead, including abrupt Core termination and descendants.
pub struct ChildJob(HANDLE);
impl Drop for ChildJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn terminate_child_with_owner(child: &std::process::Child) -> io::Result<ChildJob> {
    use windows_sys::Win32::System::JobObjects::*;
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let guard = ChildJob(handle);
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            mem::size_of_val(&limits) as u32,
        )
    } == 0
        || unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn private_unicode_files_lock_and_publish_without_overwriting() {
        let root =
            std::env::temp_dir().join(format!("loppi Windows 测试 {}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let result = (|| -> io::Result<()> {
            let pending = root.join("pending");
            let published = root.join("published");
            let mut first = OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&pending)?;
            first.write_all(b"synthetic")?;
            first.sync_all()?;
            // Inspect the actual created file's protected DACL, rather than the input SDDL.
            unsafe {
                let (mut acl, mut descriptor) = (ptr::null_mut(), ptr::null_mut());
                assert_eq!(
                    GetSecurityInfo(
                        first.as_raw_handle(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut acl,
                        ptr::null_mut(),
                        &mut descriptor
                    ),
                    0
                );
                let mut control = 0;
                let mut revision = 0;
                assert_ne!(
                    GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
                    0
                );
                assert_ne!(control & SE_DACL_PROTECTED as u16, 0);
                assert!(!acl.is_null());
                assert_eq!((*acl).AceCount, 1);
                let mut ace = ptr::null_mut();
                assert_ne!(GetAce(acl, 0, &mut ace), 0);
                let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
                assert_eq!((*ace).Header.AceType, 0 /* ACCESS_ALLOWED_ACE_TYPE */);
                assert_ne!(
                    EqualSid(
                        (&(*ace).SidStart as *const u32).cast_mut().cast(),
                        token_user()?.as_ptr() as _
                    ),
                    0
                );
                LocalFree(descriptor);
            }
            super::super::try_lock_exclusive(&first)?;
            let second = OpenOptions::new().read(true).write(true).open(&pending)?;
            assert_eq!(
                super::super::try_lock_shared(&second).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            drop(second);
            drop(first);
            rename_exclusive(&pending, &published)?;
            std::fs::write(&pending, b"replacement")?;
            assert!(rename_exclusive(&pending, &published).is_err());
            assert_eq!(std::fs::read(&published)?, b"synthetic");
            rename(&pending, &published)?;
            assert_eq!(std::fs::read(&published)?, b"replacement");
            Ok(())
        })();
        // This is a unique, task-created temporary directory, never application data.
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }

    #[test]
    fn clock_and_peer_checks_use_real_windows_identity() {
        require_supported().unwrap();
        verify_process(std::process::id()).unwrap();
        assert!(verify_process(u32::MAX).is_err());
        let first = monotonic_ns().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let delta = monotonic_ns().unwrap() - first;
        assert!(
            (10_000_000..2_000_000_000).contains(&delta),
            "unexpected QPC delta {delta}"
        );
    }
}
