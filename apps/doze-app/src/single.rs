//! One app per doze home (Go `app.go`): the first launch holds a lock and listens on `desk/app.sock`; a second launch
//! finds the lock held, hands its arguments (a folder, `--show NAME`, a `doze://` link) to the first over the socket,
//! and exits. The first opens its window and goes where the arguments say.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use async_channel::{Receiver, Sender};
use doze_os::fs::Lock;
use doze_os::local::{LocalListener, LocalStream};

/// The first launch's hold on the home: the lock, and the arguments later launches send.
pub(crate) struct First {
    _lock: Lock,
    pub(crate) args: Receiver<Vec<String>>,
}

impl std::fmt::Debug for First {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("First").finish_non_exhaustive()
    }
}

/// What this launch is.
#[derive(Debug)]
pub(crate) enum Launch {
    /// The only one: run the app.
    First(First),
    /// Another holds the home; the arguments went to it.
    Forwarded,
}

fn paths(home: &Path) -> (PathBuf, PathBuf) {
    let dir = home.join("desk");
    (dir.join("app.lock"), dir.join("app.sock"))
}

/// Takes the home, or hands `args` to the app that has it.
///
/// # Errors
///
/// When the lock cannot be made, or another app holds it and cannot be reached.
pub(crate) fn claim(home: &Path, args: &[String]) -> std::io::Result<Launch> {
    let (lock, sock) = paths(home);
    if let Some(dir) = lock.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Held by another: hand over. An app starting (or a probe of the lock by `open`) may hold it a moment before its
    // socket listens, so the two are tried again for up to a second before giving up.
    let start = std::time::Instant::now();
    let held = loop {
        if let Some(held) = doze_os::fs::try_lock(&lock)? {
            break held;
        }
        match send(&sock, args) {
            Ok(()) => return Ok(Launch::Forwarded),
            Err(e) if start.elapsed() > std::time::Duration::from_secs(1) => return Err(e),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    // A socket left by an app that died goes: the lock says nobody listens on it.
    let _ = std::fs::remove_file(&sock);
    let listener = LocalListener::bind(&sock)?;
    let (tx, rx) = async_channel::unbounded();
    let _ = std::thread::Builder::new()
        .name("doze-app single".into())
        .spawn(move || serve(&listener, &tx));
    Ok(Launch::First(First {
        _lock: held,
        args: rx,
    }))
}

/// Sends `args` to the app listening on `sock`.
fn send(sock: &Path, args: &[String]) -> std::io::Result<()> {
    let mut s = LocalStream::connect(sock)?;
    let line = serde_json::to_string(args).map_err(std::io::Error::other)?;
    s.write_all(line.as_bytes())?;
    s.write_all(b"\n")
}

/// Hands `args` to the app that holds the home, when one does (`doze-app open`); `false` when none does, and then
/// nothing is held: the launch that follows takes the home itself.
///
/// # Errors
///
/// When the lock cannot be read, or the app that holds it cannot be reached.
pub(crate) fn hand_over(home: &Path, args: &[String]) -> std::io::Result<bool> {
    let (lock, sock) = paths(home);
    // The socket first: a listening app is asked without touching its lock (a probe of the lock would hold it for a
    // moment, and an app starting then would lose it).
    if send(&sock, args).is_ok() {
        return Ok(true);
    }
    if !lock.exists() || doze_os::fs::try_lock(&lock)?.is_some() {
        return Ok(false);
    }
    // Held, not listening yet: an app that is starting. It binds the socket just after it takes the lock.
    let start = std::time::Instant::now();
    loop {
        match send(&sock, args) {
            Ok(()) => return Ok(true),
            Err(e) if start.elapsed() > std::time::Duration::from_secs(1) => return Err(e),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// Reads one line of arguments from each launch that connects.
fn serve(listener: &LocalListener, tx: &Sender<Vec<String>>) {
    while let Ok(s) = listener.accept() {
        let mut line = String::new();
        if BufReader::new(s).read_line(&mut line).is_ok()
            && let Ok(args) = serde_json::from_str::<Vec<String>>(&line)
            && tx.send_blocking(args).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Launch, claim};

    #[test]
    fn a_second_launch_hands_its_arguments_to_the_first() {
        let home = std::env::temp_dir().join(format!("doze-app-single-{}", std::process::id()));
        let first = claim(&home, &[]).unwrap();
        let Launch::First(f) = first else {
            panic!("the first launch is first")
        };
        let args = vec!["doze://shop/db".to_string(), "--show".into(), "api".into()];
        assert!(matches!(claim(&home, &args).unwrap(), Launch::Forwarded));
        assert_eq!(f.args.recv_blocking().unwrap(), args);
        drop(f);
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[cfg(test)]
mod hand_over_tests {
    #![allow(clippy::unwrap_used)]
    use super::{Launch, claim, hand_over};

    #[test]
    fn open_hands_over_to_a_running_app_and_says_when_there_is_none() {
        let home = std::env::temp_dir().join(format!("doze-app-handover-{}", std::process::id()));
        let none = vec!["/code/shop".to_string()];
        assert!(!hand_over(&home, &none).unwrap(), "no app: nothing held");
        let Ok(Launch::First(f)) = claim(&home, &[]) else {
            unreachable!("the first launch is first")
        };
        assert!(hand_over(&home, &none).unwrap());
        assert_eq!(f.args.recv_blocking().unwrap(), none);
        // The app quits: its lock goes with it, and its socket (here the listening thread outlives the drop, so the
        // socket is taken away as the process's exit would).
        drop(f);
        let _ = std::fs::remove_file(home.join("desk/app.sock"));
        assert!(
            !hand_over(&home, &none).unwrap(),
            "the app quit: its lock is free"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
