//! Windows: a protected DACL for the user and SYSTEM; cloning is a plain recursive copy (block cloning on ReFS and
//! Dev Drive is a later optimisation).
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use crate::winsec::{acl_for, current_user, system};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Security::ACL;
use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
};

pub(super) fn file_id(_meta: &std::fs::Metadata) -> u64 {
    0
}

pub(super) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Empty first, private second, contents last: the bytes never sit in a file others can read.
    std::fs::write(path, [])?;
    make_private(path)?;
    std::fs::write(path, bytes)
}

pub(super) fn make_private(path: &Path) -> io::Result<()> {
    let user = current_user()?;
    let system = system()?;
    let mut acl = acl_for(&[user.sid(), system.0])?;
    let mut name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let what = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    let (none, acl_ptr) = (std::ptr::null_mut(), acl.as_mut_ptr().cast::<ACL>());
    // SAFETY: `name` is a NUL-terminated wide path and `acl` a valid ACL, both alive for the call; owner, group and SACL
    // are left unchanged (null).
    let r = unsafe {
        SetNamedSecurityInfoW(
            name.as_mut_ptr(),
            SE_FILE_OBJECT,
            what,
            none,
            none,
            acl_ptr,
            std::ptr::null(),
        )
    };
    if r != 0 {
        return Err(io::Error::from_raw_os_error(r as i32));
    }
    Ok(())
}

pub(super) fn clone_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            clone_dir(&entry.path(), &to)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &to)?;
        } else {
            return Err(io::Error::other(format!(
                "{}: links are not copied",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

pub(super) fn make_executable(_: &Path) -> io::Result<()> {
    Ok(())
}
