//! A restricted token keeps the elevated token's default owner and default DACL, which grant access through the
//! Administrators group. With that group deny-only, the process cannot open objects it creates itself (pipes, Job
//! objects): "Access is denied" on the elevated CI runner. PostgreSQL's pg_ctl fixes the same with
//! `AddUserToTokenDacl`. Here the token gets the user as owner of what it creates, and a default DACL granting the user
//! and SYSTEM full access (files under the user's profile inherit their own ACLs and are not affected).
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use super::windows::{Sid, nt_sid};
use std::ffi::c_void;
use std::io;
use windows_sys::Win32::Foundation::{GENERIC_ALL, HANDLE};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid, GetTokenInformation,
    InitializeAcl, PSID, SetTokenInformation, TOKEN_DEFAULT_DACL, TOKEN_OWNER, TOKEN_USER,
    TokenDefaultDacl, TokenOwner, TokenUser,
};
use windows_sys::Win32::System::SystemServices::SECURITY_LOCAL_SYSTEM_RID;

/// The token's user, in a buffer aligned for TOKEN_USER (the SID it points to lives in the same buffer).
struct TokenUserBuf(Vec<u64>);

impl TokenUserBuf {
    fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER written by GetTokenInformation, 8-byte aligned, at least that large.
        let tu = unsafe { std::ptr::read(self.0.as_ptr().cast::<TOKEN_USER>()) };
        tu.User.Sid
    }
}

fn token_user(token: HANDLE) -> io::Result<TokenUserBuf> {
    let mut need: u32 = 0;
    // SAFETY: a null buffer of length 0 asks only for the size, written to `need`; the expected failure is ignored.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut need) };
    let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
    let len = u32::try_from(buf.len() * 8).map_err(io::Error::other)?;
    // SAFETY: `buf` is writable for `len` bytes and 8-byte aligned; success is checked below.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr().cast::<c_void>(),
            len,
            &mut need,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(TokenUserBuf(buf))
}

fn sid_len(sid: PSID) -> u32 {
    // SAFETY: `sid` points to a valid SID (from the token buffer or AllocateAndInitializeSid) that outlives the call.
    unsafe { GetLengthSid(sid) }
}

/// An ACL granting GENERIC_ALL to each SID, in a 4-byte-aligned buffer (u64 storage).
fn acl_for(sids: &[PSID]) -> io::Result<Vec<u64>> {
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

fn set<T>(token: HANDLE, class: i32, value: &T) -> io::Result<()> {
    let len = u32::try_from(std::mem::size_of::<T>()).map_err(io::Error::other)?;
    // SAFETY: `value` is the structure this information class expects, alive for the call, with its exact size.
    let ok =
        unsafe { SetTokenInformation(token, class, (value as *const T).cast::<c_void>(), len) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes objects created under `token` owned by, and open to, its user (and SYSTEM).
pub(super) fn grant_user(token: HANDLE) -> io::Result<()> {
    let user = token_user(token)?;
    let system: Sid = nt_sid(1, SECURITY_LOCAL_SYSTEM_RID as u32, 0)?;
    let mut acl = acl_for(&[user.sid(), system.0])?;
    set(
        token,
        TokenDefaultDacl,
        &TOKEN_DEFAULT_DACL {
            DefaultDacl: acl.as_mut_ptr().cast::<ACL>(),
        },
    )?;
    set(token, TokenOwner, &TOKEN_OWNER { Owner: user.sid() })
}
