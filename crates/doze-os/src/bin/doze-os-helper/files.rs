//! Helper modes for the fs, home and detach tests.

use doze_os::tree::{Stop, Tree};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

/// `hold-lock <path>`: takes the lock, says so, and keeps it until killed.
pub(crate) fn hold_lock(args: &[String]) -> std::io::Result<()> {
    let [path] = args else {
        return Err(std::io::Error::other("hold-lock <path>"));
    };
    let _lock = doze_os::fs::lock(Path::new(path))?;
    println!("locked");
    std::io::stdout().flush()?;
    std::thread::sleep(Duration::from_secs(3600));
    Ok(())
}

/// `home`: prints the doze home directory.
pub(crate) fn home() -> std::io::Result<()> {
    println!("{}", doze_os::home::doze_home()?.display());
    Ok(())
}

/// `launch-daemon <outfile>`: starts `daemon <outfile>` detached and exits at once, as `doze up` would.
pub(crate) fn launch_daemon(args: &[String]) -> std::io::Result<()> {
    let [out] = args else {
        return Err(std::io::Error::other("launch-daemon <outfile>"));
    };
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(["daemon", out]);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let pid = doze_os::detach::spawn_detached(cmd)?;
    println!("{pid}");
    Ok(())
}

/// `daemon <outfile>`: what a detached daemon must still manage: start an engine tree and stop it gently. Writes how
/// the stop ended to `outfile` (written last, so its presence means the daemon outlived its launcher).
pub(crate) async fn daemon(args: &[String]) -> std::io::Result<()> {
    let [out] = args else {
        return Err(std::io::Error::other("daemon <outfile>"));
    };
    // Give the launcher time to exit first.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut cmd = tokio::process::Command::new(std::env::current_exe()?);
    cmd.arg("wait-stop")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    let mut tree = Tree::spawn(cmd)?;
    if let Some(o) = tree.take_stdout() {
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut tokio::io::BufReader::new(o), &mut line).await?;
    }
    let r = tree.stop(&Stop::Terminate, Duration::from_secs(10)).await?;
    std::fs::write(out, format!("{r:?}"))
}
