//! Windows: each tree has its own kill-on-close Job object (closing the handle, including when doze dies, ends every
//! process in it) and its own console process group, so CTRL_BREAK reaches only this tree.
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use super::Stop;
use std::io;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};

#[derive(Debug)]
pub(super) struct Tree {
    job: Job,
    pid: u32,
}

pub(super) fn spawn(mut cmd: tokio::process::Command) -> io::Result<(tokio::process::Child, Tree)> {
    // Started suspended, put in its job, then let run: nothing it starts can escape the job (audit 2, minor 5).
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    let job = Job::new()?;
    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    let placed = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("child already reaped"))
        .and_then(|handle| job.assign(handle as HANDLE))
        .and_then(|()| resume(pid));
    if let Err(e) = placed {
        let _ = child.start_kill();
        return Err(e);
    }
    Ok((child, Tree { job, pid }))
}

/// Resumes the one thread of a process started suspended.
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: plain values in; the handle returned is checked and closed below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap_or(28),
        ..THREADENTRY32::default()
    };
    let mut resumed = false;
    // SAFETY: `snap` is a live snapshot handle and `entry` a live, sized THREADENTRY32 the calls fill in.
    let mut more = unsafe { Thread32First(snap, &mut entry) } != 0;
    while more && !resumed {
        if entry.th32OwnerProcessID == pid {
            resumed = resume_thread(entry.th32ThreadID)?;
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snap, &mut entry) } != 0;
    }
    // SAFETY: `snap` was opened above and is closed exactly once.
    unsafe { CloseHandle(snap) };
    if resumed {
        Ok(())
    } else {
        Err(io::Error::other(
            "the started process has no thread to resume",
        ))
    }
}

fn resume_thread(tid: u32) -> io::Result<bool> {
    // SAFETY: plain values in; the handle returned is checked and closed below.
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, tid) };
    if thread.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `thread` is a live thread handle opened with THREAD_SUSPEND_RESUME.
    let r = unsafe { ResumeThread(thread) };
    // SAFETY: `thread` was opened above and is closed exactly once.
    unsafe { CloseHandle(thread) };
    if r == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    Ok(true)
}

impl Tree {
    pub(super) fn signal(&self, _how: &Stop) -> io::Result<()> {
        // SAFETY: plain call; `pid` names a process started with CREATE_NEW_PROCESS_GROUP, so it is its own group.
        let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.pid) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Windows has no second stage: the CTRL_BREAK already sent stands, then the kill.
    pub(super) fn quit(&self) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        // SAFETY: the job handle is live (owned by `self.job`); exit code 1 for every process in it.
        let ok = unsafe { TerminateJobObject(self.job.0, 1) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// A Job object that kills its processes when its last handle closes.
#[derive(Debug)]
struct Job(HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread; we only pass it to thread-safe Win32 calls.
unsafe impl Send for Job {}
// SAFETY: as above; nothing mutates through a shared reference.
unsafe impl Sync for Job {}

impl Job {
    fn new() -> io::Result<Job> {
        // SAFETY: null attributes and name create an anonymous job; the returned handle is checked below.
        let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = Job(h);
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is valid (integers and sizes only).
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(std::mem::size_of_val(&info)).map_err(io::Error::other)?;
        let ptr = (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast();
        // SAFETY: `info` is the structure this information class expects, alive for the call, with its exact size.
        let ok =
            unsafe { SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, ptr, size) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn assign(&self, process: HANDLE) -> io::Result<()> {
        // SAFETY: both handles are live: our job, and the child's process handle that tokio holds.
        let ok = unsafe { AssignProcessToJobObject(self.0, process) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle and close it exactly once; kill-on-close ends the tree.
        unsafe { CloseHandle(self.0) };
    }
}
