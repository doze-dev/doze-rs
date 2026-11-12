//! Winsock must be started before a socket is made from another process's protocol info (std only starts it when
//! std itself first touches the network).
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use std::io;
use std::sync::OnceLock;
use windows_sys::Win32::Networking::WinSock::{WSADATA, WSAStartup};

static STARTED: OnceLock<i32> = OnceLock::new();

pub(crate) fn init() -> io::Result<()> {
    let r = *STARTED.get_or_init(|| {
        // SAFETY: an all-zero WSADATA is valid; WSAStartup fills it.
        let mut data: WSADATA = unsafe { std::mem::zeroed() };
        // SAFETY: `data` is a valid out-pointer; 0x0202 asks for Winsock 2.2. It is never cleaned up: the process keeps
        // Winsock for its whole life, as std does.
        unsafe { WSAStartup(0x0202, &mut data) }
    });
    if r != 0 {
        return Err(io::Error::from_raw_os_error(r));
    }
    Ok(())
}
