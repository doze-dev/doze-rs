//! A restricted token keeps the elevated token's default owner and default DACL, which grant access through the
//! Administrators group. With that group deny-only, the process cannot open objects it creates itself (pipes, Job
//! objects): "Access is denied" on the elevated CI runner. PostgreSQL's pg_ctl fixes the same with
//! `AddUserToTokenDacl`. Here the token gets the user as owner of what it creates, and a default DACL granting the user
//! and SYSTEM full access (files under the user's profile inherit their own ACLs and are not affected).
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use crate::winsec::{acl_for, system, token_user};
use std::ffi::c_void;
use std::io;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::{
    ACL, SetTokenInformation, TOKEN_DEFAULT_DACL, TOKEN_OWNER, TokenDefaultDacl, TokenOwner,
};

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
    let system = system()?;
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
