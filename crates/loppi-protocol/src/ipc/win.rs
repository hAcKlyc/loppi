use crate::platform::{Security, session_id, token_user, verify_process, wide};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    mem,
    path::Path,
    ptr,
    sync::Mutex,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::*,
    System::{IO::*, Pipes::*, Threading::*},
};

const LIMIT: usize = 48 * 1024 * 1024;
fn pipe_name(path: &Path) -> io::Result<Vec<u16>> {
    let user = token_user()?;
    let session = session_id(std::process::id())?;
    // Canonical parent makes distinct Unicode/space spellings resolve to one owner.
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("IPC parent missing"))?;
    let parent = std::fs::canonicalize(parent)?;
    let text = parent.to_string_lossy().to_lowercase();
    let hash = Sha256::digest([user.as_slice(), text.as_bytes()].concat());
    Ok(wide(format!(r"\\.\pipe\loppi-{session}-{hash:x}")))
}
struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Operation {
    over: Box<OVERLAPPED>,
    event: Handle,
}
impl Operation {
    fn new() -> io::Result<Self> {
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut over: Box<OVERLAPPED> = Box::new(unsafe { mem::zeroed() });
        over.hEvent = event;
        Ok(Self {
            over,
            event: Handle(event),
        })
    }
    fn finish(&mut self, handle: HANDLE, timeout: Duration) -> io::Result<u32> {
        let wait = unsafe {
            WaitForSingleObject(
                self.event.0,
                timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        };
        if wait != WAIT_OBJECT_0 {
            // OVERLAPPED and its buffer must remain alive until cancellation completes.
            unsafe {
                CancelIoEx(handle, &*self.over);
                WaitForSingleObject(self.event.0, INFINITE);
            }
            return Err(io::Error::new(
                if wait == WAIT_TIMEOUT {
                    io::ErrorKind::TimedOut
                } else {
                    io::ErrorKind::Other
                },
                "IPC operation timed out or wait failed",
            ));
        }
        let mut count = 0;
        if unsafe { GetOverlappedResult(handle, &*self.over, &mut count, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(count)
    }
}
pub struct Stream {
    handle: Handle,
    read_timeout: Mutex<Duration>,
    write_timeout: Mutex<Duration>,
    remaining: Option<usize>,
    read_deadline: Option<std::time::Instant>,
}
impl Stream {
    fn new(handle: HANDLE) -> Self {
        Self {
            handle: Handle(handle),
            read_timeout: Mutex::new(Duration::from_secs(5)),
            write_timeout: Mutex::new(Duration::from_secs(5)),
            remaining: None,
            read_deadline: None,
        }
    }
    pub fn connect(path: impl AsRef<Path>) -> io::Result<Self> {
        let name = pipe_name(path.as_ref())?;
        // A connected instance can be occupied until accept pre-arms its
        // replacement. Only wait before opening a connection; never replay I/O.
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        let handle = loop {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                break handle;
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(error);
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC connection timed out",
                ));
            }
            let milliseconds = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
            if unsafe { WaitNamedPipeW(name.as_ptr(), milliseconds) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "IPC connection timed out",
                    ));
                }
                return Err(error);
            }
        };
        let stream = Self::new(handle);
        let mut pid = 0;
        if unsafe { GetNamedPipeServerProcessId(handle, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        verify_process(pid)?;
        Ok(stream)
    }
    pub fn peer_pid(&self) -> io::Result<u32> {
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle.0, &mut pid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        verify_process(pid)?;
        Ok(pid)
    }
    pub fn set_read_timeout(&self, time: Option<Duration>) -> io::Result<()> {
        *self
            .read_timeout
            .lock()
            .map_err(|_| io::Error::other("IPC timeout lock"))? =
            time.unwrap_or(Duration::from_secs(180));
        Ok(())
    }
    pub fn set_write_timeout(&self, time: Option<Duration>) -> io::Result<()> {
        *self
            .write_timeout
            .lock()
            .map_err(|_| io::Error::other("IPC timeout lock"))? =
            time.unwrap_or(Duration::from_secs(180));
        Ok(())
    }
    pub fn set_nonblocking(&self, _nonblocking: bool) -> io::Result<()> {
        Ok(())
    }
    /// The length prefix closes one request direction without closing the duplex handle.
    pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
        Ok(())
    }
    fn transfer(
        &self,
        bytes: *mut u8,
        len: usize,
        write: bool,
        timeout: Duration,
    ) -> io::Result<usize> {
        let mut operation = Operation::new()?;
        let mut count = 0;
        let ok = unsafe {
            if write {
                WriteFile(
                    self.handle.0,
                    bytes.cast(),
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut *operation.over,
                )
            } else {
                ReadFile(
                    self.handle.0,
                    bytes.cast(),
                    len.min(u32::MAX as usize) as u32,
                    &mut count,
                    &mut *operation.over,
                )
            }
        };
        if ok != 0 {
            return Ok(count as usize);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        operation.finish(self.handle.0, timeout).map(|n| n as usize)
    }
    fn exact(
        &self,
        mut bytes: &mut [u8],
        write: bool,
        deadline: std::time::Instant,
    ) -> io::Result<()> {
        while !bytes.is_empty() {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC frame timed out",
                ));
            }
            let n = self.transfer(
                bytes.as_mut_ptr(),
                bytes.len(),
                write,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "IPC frame interrupted",
                ));
            }
            bytes = &mut bytes[n..];
        }
        Ok(())
    }
}
impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.remaining.is_none() {
            let deadline = std::time::Instant::now() + *self.read_timeout.lock().unwrap();
            self.read_deadline = Some(deadline);
            let mut prefix = [0; 4];
            self.exact(&mut prefix, false, deadline)?;
            let len = u32::from_le_bytes(prefix) as usize;
            if len > LIMIT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "IPC frame exceeded limit",
                ));
            }
            self.remaining = Some(len);
        }
        let remaining = self.remaining.unwrap();
        if remaining == 0 {
            return Ok(0);
        }
        let timeout = self
            .read_deadline
            .unwrap()
            .saturating_duration_since(std::time::Instant::now())
            .min(*self.read_timeout.lock().unwrap());
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC frame timed out",
            ));
        }
        let n = self.transfer(
            bytes.as_mut_ptr(),
            bytes.len().min(remaining),
            false,
            timeout,
        )?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "IPC frame interrupted",
            ));
        }
        self.remaining = Some(remaining - n);
        Ok(n)
    }
}
impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "IPC frame exceeded limit",
            ));
        }
        let deadline = std::time::Instant::now() + *self.write_timeout.lock().unwrap();
        let mut prefix = (bytes.len() as u32).to_le_bytes();
        self.exact(&mut prefix, true, deadline)?;
        // WriteFile does not mutate an output buffer; no allocation/copy of private payloads.
        let mut offset = 0;
        while offset < bytes.len() {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "IPC write timed out",
                ));
            }
            let n = self.transfer(
                bytes[offset..].as_ptr() as _,
                bytes.len() - offset,
                true,
                deadline.saturating_duration_since(std::time::Instant::now()),
            )?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "IPC disconnected"));
            }
            offset += n;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Pending {
    handle: Handle,
    operation: Operation,
    connected: bool,
}
unsafe impl Send for Pending {}
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.connected {
            unsafe {
                CancelIoEx(self.handle.0, &*self.operation.over);
                WaitForSingleObject(self.operation.event.0, INFINITE);
            }
        }
    }
}
pub struct Listener {
    name: Vec<u16>,
    pending: Mutex<Option<Pending>>,
}
impl Listener {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let name = pipe_name(path.as_ref())?;
        let pending = Self::create(&name, true)?;
        Ok(Self {
            name,
            pending: Mutex::new(Some(pending)),
        })
    }
    fn create(name: &[u16], first: bool) -> io::Result<Pending> {
        let security = Security::session()?;
        let attributes = security.attributes();
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX
                    | FILE_FLAG_OVERLAPPED
                    | if first {
                        FILE_FLAG_FIRST_PIPE_INSTANCE
                    } else {
                        0
                    },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                34,
                65536,
                65536,
                0,
                &attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let handle = Handle(handle);
        let mut operation = Operation::new()?;
        let ok = unsafe { ConnectNamedPipe(handle.0, &mut *operation.over) };
        let error = unsafe { GetLastError() };
        let connected = ok != 0 || error == ERROR_PIPE_CONNECTED;
        if !connected && error != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(Pending {
            handle,
            operation,
            connected,
        })
    }
    pub fn set_nonblocking(&self, _value: bool) -> io::Result<()> {
        Ok(())
    }
    pub fn accept(&self) -> io::Result<(Stream, ())> {
        let mut guard = self
            .pending
            .lock()
            .map_err(|_| io::Error::other("IPC listener lock"))?;
        let pending = guard
            .as_mut()
            .ok_or_else(|| io::Error::other("IPC listener unavailable"))?;
        if !pending.connected {
            if unsafe { WaitForSingleObject(pending.operation.event.0, 0) } != WAIT_OBJECT_0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let mut count = 0;
            if unsafe {
                GetOverlappedResult(pending.handle.0, &*pending.operation.over, &mut count, 0)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            pending.connected = true;
        }
        let replacement = Self::create(&self.name, false)?;
        let mut connected = guard.replace(replacement).unwrap();
        let handle = mem::replace(&mut connected.handle.0, ptr::null_mut());
        Ok((Stream::new(handle), ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Root(std::path::PathBuf);
    impl Root {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("loppi pipe 测试 {}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            std::fs::remove_dir(&self.0).unwrap();
        }
    }
    fn accept(listener: &Listener) -> Stream {
        let start = std::time::Instant::now();
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    assert!(start.elapsed() < Duration::from_secs(2));
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("{e}"),
            }
        }
    }

    #[test]
    fn actual_pipe_identity_frames_and_exclusive_namespace() {
        let root = Root::new();
        let path = root.0.join("core.sock");
        let listener = Listener::bind(&path).unwrap();
        assert!(Listener::bind(&path).is_err());
        let mut client = Stream::connect(&path).unwrap();
        let mut server = accept(&listener);
        assert_eq!(server.peer_pid().unwrap(), std::process::id());
        client.write_all(b"synthetic request").unwrap();
        let mut payload = Vec::new();
        server.read_to_end(&mut payload).unwrap();
        assert_eq!(payload, b"synthetic request");
        server.write_all(b"synthetic response").unwrap();
        payload.clear();
        client.read_to_end(&mut payload).unwrap();
        assert_eq!(payload, b"synthetic response");
    }

    #[test]
    fn occupied_instance_waits_for_accept_to_prearm_replacement() {
        let root = Root::new();
        let path = root.0.join("core.sock");
        let listener = Listener::bind(&path).unwrap();
        let first = Stream::connect(&path).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || send.send(Stream::connect(path)).unwrap());
        assert!(matches!(
            receive.recv_timeout(Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        let _first_server = accept(&listener);
        let mut second = receive
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        waiter.join().unwrap();
        let mut second_server = accept(&listener);
        second.write_all(b"one request after connecting").unwrap();
        let mut payload = Vec::new();
        second_server.read_to_end(&mut payload).unwrap();
        assert_eq!(payload, b"one request after connecting");
        drop(first);
    }

    #[test]
    fn occupied_instance_has_bounded_wait_and_offline_errors_stay_immediate() {
        let root = Root::new();
        let path = root.0.join("core.sock");
        let start = std::time::Instant::now();
        let error = Stream::connect(&path).err().unwrap();
        assert_eq!(error.raw_os_error(), Some(ERROR_FILE_NOT_FOUND as i32));
        assert!(start.elapsed() < Duration::from_millis(250));
        let _listener = Listener::bind(&path).unwrap();
        let _occupied = Stream::connect(&path).unwrap();
        let start = std::time::Instant::now();
        let error = Stream::connect(&path).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert!(start.elapsed() < Duration::from_millis(750));
    }

    #[test]
    fn incomplete_prefix_has_one_deadline_and_cancels_pending_io() {
        let root = Root::new();
        let path = root.0.join("core.sock");
        let listener = Listener::bind(&path).unwrap();
        let client = Stream::connect(&path).unwrap();
        let mut server = accept(&listener);
        server
            .set_read_timeout(Some(Duration::from_millis(160)))
            .unwrap();
        let writer = std::thread::spawn(move || {
            for byte in [12u8, 0, 0, 0] {
                let _ =
                    client.transfer(&byte as *const u8 as _, 1, true, Duration::from_millis(100));
                std::thread::sleep(Duration::from_millis(90));
            }
        });
        let start = std::time::Instant::now();
        let error = server.read(&mut [0; 32]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_millis(450));
        drop(server);
        writer.join().unwrap();
    }

    #[test]
    fn oversized_prefix_is_rejected_before_payload_allocation() {
        let root = Root::new();
        let path = root.0.join("core.sock");
        let listener = Listener::bind(&path).unwrap();
        let client = Stream::connect(&path).unwrap();
        let mut server = accept(&listener);
        let prefix = (LIMIT as u32 + 1).to_le_bytes();
        client
            .transfer(prefix.as_ptr() as _, 4, true, Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            server.read(&mut [0; 4]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
