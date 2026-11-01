//! Local transport. Windows uses one bounded frame per direction on a restricted
//! named pipe; macOS retains its existing Unix stream wire format.
#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::*;
