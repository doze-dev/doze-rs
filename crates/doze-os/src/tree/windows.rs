//! Windows: each tree has its own kill-on-close Job object (closing the handle, including when doze dies, ends every
//! process in it) and its own console process group, so CTRL_BREAK reaches only this tree.
// Win32 calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use super::Stop;
use std::io;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

#[derive(Debug)]
pub(super) struct Tree {
    job: Job,
    pid: u32,
}

pub(super) fn spawn(mut cmd: tokio::process::Command) -> io::Result<(tokio::process::Child, Tree)> {
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    let job = Job::new()?;
    let child = cmd.spawn()?;
    // Assigned right after start: a process that starts a child within microseconds could escape this job (p1.md);
    // the daemon's own job still catches it when doze exits.
    let handle = child
        .raw_handle()
        .ok_or_else(|| io::Error::other("child already reaped"))?;
    job.assign(handle as HANDLE)?;
    let pid = child.id().unwrap_or(0);
    Ok((child, Tree { job, pid }))
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
