//! OS operations shared by GUI, CLI, Core and workers. No business state lives here.
use std::{io, path::Path, process::Command};

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, including capture/model helpers.
    }
    command
}

pub fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}

/// Database keys use '/' on every OS. Only generated relative application paths
/// belong here; filesystem access still uses Path and its existing validation.
pub fn relative_key(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn lock_error(error: io::Error) -> io::Error {
    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
        io::Error::new(io::ErrorKind::WouldBlock, error)
    } else {
        error
    }
}
pub fn try_lock_exclusive(file: &std::fs::File) -> io::Result<()> {
    fs2::FileExt::try_lock_exclusive(file).map_err(lock_error)
}
pub fn try_lock_shared(file: &std::fs::File) -> io::Result<()> {
    fs2::FileExt::try_lock_shared(file).map_err(lock_error)
}

#[cfg(unix)]
pub use std::fs::OpenOptions;
#[cfg(unix)]
pub use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

#[cfg(unix)]
pub fn set_mode(path: impl AsRef<Path>, mode: u32) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(unix)]
pub fn open_directory(path: impl AsRef<Path>) -> io::Result<std::fs::File> {
    std::fs::File::open(path)
}
#[cfg(unix)]
pub fn sync_directory(path: impl AsRef<Path>) -> io::Result<()> {
    open_directory(path)?.sync_all()
}
#[cfg(unix)]
pub fn volume_id(path: impl AsRef<Path>) -> io::Result<u64> {
    Ok(std::fs::metadata(path)?.dev())
}
#[cfg(unix)]
pub use std::fs::rename;
#[cfg(unix)]
pub fn sync_file(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}
#[cfg(unix)]
pub const NOFOLLOW: i32 = 0x00000004 | 0x00000100; // O_NONBLOCK | O_NOFOLLOW on macOS.
#[cfg(unix)]
pub fn parent_pid() -> i32 {
    unsafe { libc_parent_pid() }
}
#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getppid"]
    fn libc_parent_pid() -> i32;
}
#[cfg(unix)]
pub fn parent_alive(pid: i32) -> bool {
    parent_pid() == pid
}
#[cfg(unix)]
pub fn hard_exit(code: i32) -> ! {
    unsafe extern "C" {
        fn _exit(code: i32) -> !;
    }
    unsafe { _exit(code) }
}

#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;
