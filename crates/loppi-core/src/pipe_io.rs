//! Bounded, cancellable anonymous-pipe I/O. PIPE_NOWAIT is supported by the
//! Windows anonymous pipe implementation; actual zero progress yields to the owner.
use std::{
    io,
    os::windows::io::AsRawHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Pipes::*};
pub fn nonblocking(pipe: &impl AsRawHandle) -> io::Result<()> {
    let mode = PIPE_NOWAIT;
    if unsafe {
        SetNamedPipeHandleState(
            pipe.as_raw_handle(),
            &mode,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn try_read(pipe: &impl AsRawHandle, bytes: &mut [u8]) -> io::Result<usize> {
    let mut available = 0;
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let e = io::Error::last_os_error();
        return if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            Ok(0)
        } else {
            Err(e)
        };
    }
    if available == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let mut count = 0;
    if unsafe {
        ReadFile(
            pipe.as_raw_handle(),
            bytes.as_mut_ptr(),
            bytes.len().min(available as usize) as u32,
            &mut count,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(count as usize)
}
pub fn try_write(pipe: &impl AsRawHandle, bytes: &[u8]) -> io::Result<usize> {
    nonblocking(pipe)?;
    let mut count = 0;
    if unsafe {
        WriteFile(
            pipe.as_raw_handle(),
            bytes.as_ptr(),
            bytes.len().min(u32::MAX as usize) as u32,
            &mut count,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_DATA as i32) {
            Err(io::ErrorKind::WouldBlock.into())
        } else {
            Err(error)
        };
    }
    if count == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    Ok(count as usize)
}
fn tick(deadline: Instant, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        return Err("resource_deferred".into());
    }
    if Instant::now() >= deadline {
        return Err("worker_timeout".into());
    }
    Ok(())
}
pub fn read(
    pipe: &impl AsRawHandle,
    mut bytes: &mut [u8],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    while !bytes.is_empty() {
        tick(deadline, cancelled)?;
        match try_read(pipe, bytes) {
            Ok(0) => return Err("worker_failed".into()),
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => return Err("worker_failed".into()),
        }
    }
    Ok(())
}
pub fn write(
    pipe: &impl AsRawHandle,
    mut bytes: &[u8],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    while !bytes.is_empty() {
        tick(deadline, cancelled)?;
        match try_write(pipe, bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => return Err("worker_failed".into()),
        }
    }
    Ok(())
}
