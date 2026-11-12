//! Windows: membership checks and the restricted-token re-launch (the same token `pg_ctl` builds).
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, CheckTokenMembership, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE,
    FreeSid, PSID, SECURITY_NT_AUTHORITY, SID_AND_ATTRIBUTES, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Environment::GetCommandLineW;
use windows_sys::Win32::System::SystemServices::{
    DOMAIN_ALIAS_RID_ADMINS, DOMAIN_ALIAS_RID_POWER_USERS, SECURITY_BUILTIN_DOMAIN_RID,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess,
    INFINITE, OpenProcessToken, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
    WaitForSingleObject,
};

/// A SID from `AllocateAndInitializeSid`, freed on drop.
pub(super) struct Sid(pub(super) PSID);

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the SID came from AllocateAndInitializeSid and is freed exactly once.
        unsafe { FreeSid(self.0) };
    }
}

fn builtin_group(rid: i32) -> io::Result<Sid> {
    nt_sid(2, SECURITY_BUILTIN_DOMAIN_RID as u32, rid as u32)
}

/// A SID under the NT authority with one or two sub-authorities (the second is ignored when `count` is 1).
pub(super) fn nt_sid(count: u8, first: u32, second: u32) -> io::Result<Sid> {
    let mut sid: PSID = std::ptr::null_mut();
    let (domain, rid) = (first, second);
    // SAFETY: the authority is a valid constant; `sid` is a valid out-pointer; success is checked below.
    let ok = unsafe {
        AllocateAndInitializeSid(
            &SECURITY_NT_AUTHORITY,
            count,
            domain,
            rid,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut sid,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Sid(sid))
}

fn privileged_groups() -> io::Result<[Sid; 2]> {
    Ok([
        builtin_group(DOMAIN_ALIAS_RID_ADMINS)?,
        builtin_group(DOMAIN_ALIAS_RID_POWER_USERS)?,
    ])
}

pub(super) fn is_privileged() -> io::Result<bool> {
    for sid in privileged_groups()? {
        let mut member = 0;
        // SAFETY: a null token means "this thread's effective token"; the SID is live; `member` is an out-pointer.
        let ok = unsafe { CheckTokenMembership(std::ptr::null_mut(), sid.0, &mut member) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if member != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn owned(h: HANDLE) -> OwnedHandle {
    // SAFETY: called only on handles a successful Win32 call just returned to us, and nothing else owns them.
    unsafe { OwnedHandle::from_raw_handle(h) }
}

fn restricted_token() -> io::Result<OwnedHandle> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing; it has no preconditions.
    let me = unsafe { GetCurrentProcess() };
    // SAFETY: `me` is valid for this process; `token` is a valid out-pointer; success is checked below.
    let ok = unsafe {
        OpenProcessToken(
            me,
            TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
            &mut token,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = owned(token);
    let groups = privileged_groups()?;
    let disable = groups.each_ref().map(|s| SID_AND_ATTRIBUTES {
        Sid: s.0,
        Attributes: 0,
    });
    let mut restricted: HANDLE = std::ptr::null_mut();
    let src = std::os::windows::io::AsRawHandle::as_raw_handle(&token) as HANDLE;
    // SAFETY: `src` is our live token; `disable` holds two live SIDs (`groups` outlives the call); `restricted` is an
    // out-pointer; success is checked below.
    let ok = unsafe {
        CreateRestrictedToken(
            src,
            DISABLE_MAX_PRIVILEGE,
            2,
            disable.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut restricted,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let restricted = owned(restricted);
    super::dacl::grant_user(
        std::os::windows::io::AsRawHandle::as_raw_handle(&restricted) as HANDLE,
    )?;
    Ok(restricted)
}

/// This process's command line, as a writable, NUL-terminated buffer (CreateProcessAsUserW may write to it).
fn command_line() -> Vec<u16> {
    // SAFETY: GetCommandLineW has no preconditions and returns a pointer valid for the life of the process.
    let p = unsafe { GetCommandLineW() };
    let mut out = Vec::new();
    loop {
        let q = p.wrapping_add(out.len());
        // SAFETY: `q` is within the NUL-terminated string: we stop at the first NUL, which is always present.
        let c = unsafe { q.read() };
        out.push(c);
        if c == 0 {
            return out;
        }
    }
}

/// This process's environment plus the re-launch marker, as a CREATE_UNICODE_ENVIRONMENT block.
fn environment() -> Vec<u16> {
    let mut block = Vec::new();
    let marker = (
        std::ffi::OsString::from(super::RELAUNCHED_ENV),
        std::ffi::OsString::from("1"),
    );
    for (k, v) in std::env::vars_os()
        .filter(|(k, _)| k != super::RELAUNCHED_ENV)
        .chain(std::iter::once(marker))
    {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn std_handle(which: u32) -> HANDLE {
    // SAFETY: plain call; an invalid or null result is passed through and the child simply has no such handle.
    unsafe { GetStdHandle(which) }
}

pub(super) fn relaunch() -> io::Result<i32> {
    let token = restricted_token()?;
    let mut cmd = command_line();
    let env = environment();
    // SAFETY: an all-zero STARTUPINFOW is valid; the fields that matter are set below.
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = u32::try_from(std::mem::size_of::<STARTUPINFOW>()).map_err(io::Error::other)?;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = std_handle(STD_INPUT_HANDLE);
    si.hStdOutput = std_handle(STD_OUTPUT_HANDLE);
    si.hStdError = std_handle(STD_ERROR_HANDLE);
    // SAFETY: an all-zero PROCESS_INFORMATION is valid; CreateProcessAsUserW fills it.
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let t = std::os::windows::io::AsRawHandle::as_raw_handle(&token) as HANDLE;
    let envp = env.as_ptr().cast::<c_void>();
    let (no_app, no_attrs, no_dir) = (std::ptr::null(), std::ptr::null(), std::ptr::null());
    // SAFETY: `t` is a restricted copy of our own primary token (so no extra privilege is needed); `cmd` and `env`
    // are NUL-terminated buffers alive for the call; `si` and `pi` are valid; handles are inherited so the copy shares
    // our stdio; success is checked below.
    let ok = unsafe {
        CreateProcessAsUserW(
            t,
            no_app,
            cmd.as_mut_ptr(),
            no_attrs,
            no_attrs,
            1,
            CREATE_UNICODE_ENVIRONMENT,
            envp,
            no_dir,
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let (process, _thread) = (owned(pi.hProcess), owned(pi.hThread));
    let ph = std::os::windows::io::AsRawHandle::as_raw_handle(&process) as HANDLE;
    // SAFETY: `ph` is the live process handle we own.
    if unsafe { WaitForSingleObject(ph, INFINITE) } != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    let mut code: u32 = 0;
    // SAFETY: `ph` is live; `code` is a valid out-pointer.
    if unsafe { GetExitCodeProcess(ph, &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // Exit codes are u32 on Windows; std's exit takes i32 and passes the bits through unchanged.
    Ok(code as i32)
}
