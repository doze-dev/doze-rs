//! Windows security building blocks shared by `privilege` (the restricted token) and `fs` (owner-only directories):
//! SIDs, the current user, and an ACL that grants full access to a list of SIDs.
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{GENERIC_ALL, HANDLE};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, AllocateAndInitializeSid, FreeSid,
    GetLengthSid, GetTokenInformation, InitializeAcl, PSID, SECURITY_NT_AUTHORITY, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::SystemServices::SECURITY_LOCAL_SYSTEM_RID;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// A SID from `AllocateAndInitializeSid`, freed on drop.
pub(crate) struct Sid(pub(crate) PSID);

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the SID came from AllocateAndInitializeSid and is freed exactly once.
        unsafe { FreeSid(self.0) };
    }
}

/// A SID under the NT authority with one or two sub-authorities (the second is ignored when `count` is 1).
pub(crate) fn nt_sid(count: u8, first: u32, second: u32) -> io::Result<Sid> {
    let mut sid: PSID = std::ptr::null_mut();
    let auth = &SECURITY_NT_AUTHORITY;
    // SAFETY: the authority is a valid constant; `sid` is a valid out-pointer; success is checked below.
    let ok =
        unsafe { AllocateAndInitializeSid(auth, count, first, second, 0, 0, 0, 0, 0, 0, &mut sid) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Sid(sid))
}

/// The LocalSystem account.
pub(crate) fn system() -> io::Result<Sid> {
    nt_sid(1, SECURITY_LOCAL_SYSTEM_RID as u32, 0)
}

/// A token's user, in a buffer aligned for TOKEN_USER (the SID it points to lives in the same buffer).
pub(crate) struct TokenUserBuf(Vec<u64>);

impl TokenUserBuf {
    pub(crate) fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER written by GetTokenInformation, 8-byte aligned, at least that large.
        let tu = unsafe { std::ptr::read(self.0.as_ptr().cast::<TOKEN_USER>()) };
        tu.User.Sid
    }
}

pub(crate) fn token_user(token: HANDLE) -> io::Result<TokenUserBuf> {
    let mut need: u32 = 0;
    // SAFETY: a null buffer of length 0 asks only for the size, written to `need`; the expected failure is ignored.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut need) };
    let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
    let len = u32::try_from(buf.len() * 8).map_err(io::Error::other)?;
    let out = buf.as_mut_ptr().cast::<c_void>();
    // SAFETY: `buf` is writable for `len` bytes and 8-byte aligned; success is checked below.
    let ok = unsafe { GetTokenInformation(token, TokenUser, out, len, &mut need) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(TokenUserBuf(buf))
}

/// The user this process runs as.
pub(crate) fn current_user() -> io::Result<TokenUserBuf> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; it has no preconditions.
    let me = unsafe { GetCurrentProcess() };
    // SAFETY: `me` is valid for this process; `token` is a valid out-pointer; success is checked below.
    if unsafe { OpenProcessToken(me, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `token` was just returned by a successful OpenProcessToken and nothing else owns it.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    token_user(token.as_raw_handle() as HANDLE)
}

fn sid_len(sid: PSID) -> u32 {
    // SAFETY: `sid` points to a valid SID that outlives the call.
    unsafe { GetLengthSid(sid) }
}

/// An ACL granting GENERIC_ALL to each SID, in an aligned buffer (u64 storage). Use it through `.as_mut_ptr()`.
pub(crate) fn acl_for(sids: &[PSID]) -> io::Result<Vec<u64>> {
    let ace = std::mem::size_of::<ACCESS_ALLOWED_ACE>() - std::mem::size_of::<u32>();
    let size: usize = std::mem::size_of::<ACL>()
        + sids
            .iter()
            .map(|s| ace + sid_len(*s) as usize)
            .sum::<usize>();
    let mut buf = vec![0u64; size.div_ceil(8)];
    let acl = buf.as_mut_ptr().cast::<ACL>();
    let size = u32::try_from(size).map_err(io::Error::other)?;
    // SAFETY: `acl` points to `size` writable, aligned bytes.
    if unsafe { InitializeAcl(acl, size, ACL_REVISION) } == 0 {
        return Err(io::Error::last_os_error());
    }
    for sid in sids {
        // SAFETY: the ACL was initialized with room for these ACEs; `sid` is valid for the call.
        if unsafe { AddAccessAllowedAce(acl, ACL_REVISION, GENERIC_ALL, *sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(buf)
}
