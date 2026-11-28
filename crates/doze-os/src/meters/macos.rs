//! Reading a process on macOS through libproc: its children (`proc_listchildpids`), its resident memory and CPU time
//! (`proc_pidinfo` with `PROC_PIDTASKINFO`), and its command as `ps -o comm` shows it (the first argument, through
//! `KERN_PROCARGS2`, so a postgres backend that names itself `postgres: checkpointer` is shown so).
// libproc and sysctl calls: each unsafe block below does one thing and carries its SAFETY reason (p1.md).
#![allow(unsafe_code)]

use super::Raw;
use std::ffi::c_int;
use std::sync::OnceLock;

/// The most children one process is asked for.
const MOST_CHILDREN: usize = 16 * 1024;

/// Reads processes, with one buffer for their arguments.
#[derive(Debug)]
pub(super) struct Reader {
    args: Vec<u8>,
}

impl Reader {
    pub(super) fn new() -> Option<Reader> {
        Some(Reader {
            args: vec![0; argmax()],
        })
    }

    /// The pids whose parent is `pid`.
    pub(super) fn children(&self, pid: u32) -> Vec<u32> {
        let Ok(ppid) = libc::pid_t::try_from(pid) else {
            return Vec::new();
        };
        let mut buf: Vec<libc::pid_t> = vec![0; 64];
        loop {
            let bytes =
                c_int::try_from(std::mem::size_of_val(buf.as_slice())).unwrap_or(c_int::MAX);
            // SAFETY: `buf` holds `bytes` writable bytes, and the call writes no more than that.
            let n = unsafe { libc::proc_listchildpids(ppid, buf.as_mut_ptr().cast(), bytes) };
            // It answers with how many pids it wrote.
            let Ok(n) = usize::try_from(n) else {
                return Vec::new();
            };
            if n < buf.len() || buf.len() >= MOST_CHILDREN {
                buf.truncate(n);
                return buf
                    .into_iter()
                    .filter_map(|p| u32::try_from(p).ok())
                    .filter(|p| *p != 0)
                    .collect();
            }
            // Full: there may be more.
            buf.resize(buf.len() * 4, 0);
        }
    }

    /// `pid`'s memory, CPU time and command; `None` once it is gone (or not ours to read).
    pub(super) fn read(&mut self, pid: u32) -> Option<Raw> {
        let p = c_int::try_from(pid).ok()?;
        let size = c_int::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
        // SAFETY: proc_taskinfo is plain integers, for which all-zero bytes are a value.
        let mut ti: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        // SAFETY: `ti` is a live proc_taskinfo of `size` bytes, and the call writes no more than that.
        let got =
            unsafe { libc::proc_pidinfo(p, libc::PROC_PIDTASKINFO, 0, (&raw mut ti).cast(), size) };
        if got != size {
            return None;
        }
        Some(Raw {
            pid,
            ppid: 0,
            rss: ti.pti_resident_size,
            cpu_ns: nanos(ti.pti_total_user.saturating_add(ti.pti_total_system)),
            cmd: self.command(p).unwrap_or_default(),
        })
    }

    fn command(&mut self, pid: c_int) -> Option<String> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut len = self.args.len();
        // SAFETY: `mib` is the three names given, `args` holds `len` writable bytes and `len` is updated to what was
        // written; nothing is set (a null new value of length 0).
        let r = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                self.args.as_mut_ptr().cast(),
                &raw mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 {
            return None;
        }
        first_arg(self.args.get(..len)?).map(|a| super::command(&a))
    }
}

/// The first argument in what `KERN_PROCARGS2` gives: a count, the executable's path, padding, then the arguments,
/// each ending with a 0. The path when there are no arguments.
fn first_arg(data: &[u8]) -> Option<String> {
    let rest = data.get(std::mem::size_of::<c_int>()..)?;
    let path_end = rest.iter().position(|b| *b == 0)?;
    let path = rest.get(..path_end)?;
    let after = rest.get(path_end..)?;
    let arg = match after.iter().position(|b| *b != 0) {
        Some(start) => {
            let a = after.get(start..)?;
            a.get(..a.iter().position(|b| *b == 0).unwrap_or(a.len()))?
        }
        None => path,
    };
    let arg = if arg.is_empty() { path } else { arg };
    Some(String::from_utf8_lossy(arg).into_owned())
}

/// The kernel's largest argument area, which the buffer for one process's arguments must hold.
fn argmax() -> usize {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut max: c_int = 0;
    let mut len = std::mem::size_of::<c_int>();
    // SAFETY: `mib` is the two names given, `max` is a writable int of `len` bytes; nothing is set.
    let r = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&raw mut max).cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    match usize::try_from(max) {
        Ok(m) if r == 0 && m > 0 => m,
        _ => 1024 * 1024,
    }
}

/// Task times in nanoseconds: libproc gives them in the machine's absolute-time units, which are nanoseconds on Intel
/// and 125/3 of one on Apple silicon.
fn nanos(ticks: u64) -> u64 {
    let (numer, denom) = timebase();
    u64::try_from(u128::from(ticks) * u128::from(numer) / u128::from(denom)).unwrap_or(u64::MAX)
}

#[allow(deprecated)] // libc points at the mach2 crate for this one call; not worth a dependency
fn timebase() -> (u32, u32) {
    static TIMEBASE: OnceLock<(u32, u32)> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a live, writable mach_timebase_info.
        let r = unsafe { libc::mach_timebase_info(&raw mut info) };
        if r == 0 && info.numer != 0 && info.denom != 0 {
            (info.numer, info.denom)
        } else {
            (1, 1)
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_first_argument_is_read_after_the_path_and_its_padding() {
        let mut d = 2i32.to_ne_bytes().to_vec();
        d.extend_from_slice(b"/usr/local/bin/postgres\0\0\0\0postgres: checkpointer \0-D\0");
        assert_eq!(first_arg(&d).unwrap(), "postgres: checkpointer ");
        let mut d = 0i32.to_ne_bytes().to_vec();
        d.extend_from_slice(b"/bin/sh\0\0\0");
        assert_eq!(first_arg(&d).unwrap(), "/bin/sh");
        assert!(first_arg(b"\0\0").is_none());
    }

    #[test]
    fn children_and_cpu_time_are_read() {
        let mut kids: Vec<_> = (0..3)
            .map(|_| {
                std::process::Command::new("/bin/sleep")
                    .arg("5")
                    .spawn()
                    .unwrap()
            })
            .collect();
        let mut r = Reader::new().unwrap();
        let me = std::process::id();
        let mut got = r.children(me);
        got.sort_unstable();
        let mut want: Vec<u32> = kids.iter().map(std::process::Child::id).collect();
        want.sort_unstable();
        assert!(
            want.iter().all(|k| got.contains(k)),
            "{got:?} lacks some of {want:?}"
        );
        let k = r.read(want[0]).unwrap();
        assert_eq!(k.cmd, "sleep");
        assert!(k.rss > 0);
        // Spend a known amount of CPU: the total is in nanoseconds, whatever the machine's tick. The spin ends when the meter itself
        // reads 300 ms more, not after 300 ms of the clock: on a loaded machine the thread gets only a share of the wall time (170 ms of
        // CPU in 300 ms was seen at a load of 20). A cap on the wall time keeps a meter that never moves from hanging the test.
        const WANT_NS: u64 = 300_000_000;
        let before = r.read(me).unwrap().cpu_ns;
        let t = std::time::Instant::now();
        let mut x = 0u64;
        let spent = loop {
            for _ in 0..100_000 {
                x = std::hint::black_box(x.wrapping_add(1));
            }
            let spent = r.read(me).unwrap().cpu_ns - before;
            if spent >= WANT_NS {
                break spent;
            }
            assert!(
                t.elapsed() < std::time::Duration::from_secs(10),
                "after {:?} of wall time the process has used {spent} ns of CPU by the meter (wanted {WANT_NS} ns)",
                t.elapsed()
            );
        };
        // A meter reporting nonsense (far more than was spent) is caught here.
        assert!((WANT_NS..3_000_000_000).contains(&spent), "{spent} ns");
        for k in &mut kids {
            let _ = k.kill();
            let _ = k.wait();
        }
    }
}
