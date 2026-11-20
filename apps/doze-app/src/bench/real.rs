//! `bench --real [N]`: the idle window measured over a REAL daemon (the owner's rule: test the GUI against a real
//! daemon; a still fixture hid what a live stack does to the window). This program is the CLI and the daemon (as
//! `tests/real_daemon.rs` runs it), on a throwaway doze home under /tmp, never `~/.doze`. The stack is N services
//! (13 by default) made of the shell and `nc` alone, so any Mac runs it: listeners that doze would put to sleep after
//! an hour idle (a countdown on the page, and no fall asleep inside a run, which would be a change of state drawn at
//! once, not idling), two workers that log every two seconds, and a driver that connects to four of the listeners
//! every second (their connections, CPU and memory move all the time, as a real stack's do).

use std::path::{Path, PathBuf};

use doze_desk::act::{Cli, StackAction};

/// A stack running under its own daemon, taken down (and its folder removed) by [`Real::down`].
pub(crate) struct Real {
    root: PathBuf,
    pub(crate) home: PathBuf,
    dir: PathBuf,
    cli: Cli,
}

fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()?
        .port())
}

/// The stack's doze.hcl: `n` services, at least the driver, two workers and four listeners.
fn config(n: usize) -> std::io::Result<String> {
    let listeners = n.saturating_sub(3).max(4);
    let mut hcl = String::from("stack = \"shop\"\n\n");
    let mut ports = Vec::new();
    for i in 0..listeners {
        let port = free_port()?;
        ports.push(port);
        hcl.push_str(&format!(
            "app \"svc{i:02}\" {{\n  run   = \"exec nc -lk $HOST $PORT\"\n  port  = {port}\n  sleep = \"1h\"\n}}\n\n"
        ));
    }
    for w in ["worker", "mailer"] {
        hcl.push_str(&format!(
            "app \"{w}\" {{\n  run = \"n=0; while :; do n=$((n+1)); echo {w} job $n; sleep 2; done\"\n}}\n\n"
        ));
    }
    let hit: Vec<String> = ports
        .iter()
        .take(4)
        .map(|p| format!("nc -z 127.0.0.1 {p};"))
        .collect();
    hcl.push_str(&format!(
        "app \"driver\" {{\n  run = \"while :; do {} sleep 1; done\"\n}}\n",
        hit.join(" ")
    ));
    Ok(hcl)
}

impl Real {
    /// Writes the stack and brings it up with this program as the CLI; the daemon stays up until [`Real::down`].
    pub(crate) fn up(n: usize) -> anyhow::Result<Real> {
        // A control socket's path must fit a sockaddr (104 bytes on macOS): the system's temp folder is long there.
        let base = if Path::new("/tmp").is_dir() {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let root = base.join(format!("dz-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (home, dir) = (root.join("home"), root.join("shop"));
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&dir)?;
        let dir = std::fs::canonicalize(&dir)?;
        std::fs::write(dir.join("doze.hcl"), config(n)?)?;
        let cli = Cli {
            program: std::env::current_exe()?,
            home: home.clone(),
        };
        let real = Real {
            root,
            home,
            dir,
            cli,
        };
        if let Err(f) = real.cli.stack(&StackAction::UpAll, "shop", &real.dir) {
            real.down();
            anyhow::bail!("up failed: {} {:?}", f.problem, f.lines);
        }
        println!("real_stack_up {} services", n.saturating_sub(3).max(4) + 3);
        Ok(real)
    }

    /// Takes the stack down and removes its folder.
    pub(crate) fn down(&self) {
        let _ = self.cli.stack(&StackAction::Down, "shop", &self.dir);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stack_has_its_services_and_a_driver_on_four_listeners() {
        let hcl = super::config(13).unwrap_or_default();
        assert_eq!(hcl.matches("app \"").count(), 13);
        assert_eq!(hcl.matches("nc -z").count(), 4);
        assert!(hcl.contains("app \"worker\"") && hcl.contains("app \"mailer\""));
        assert_eq!(
            super::config(2)
                .unwrap_or_default()
                .matches("app \"")
                .count(),
            7
        );
    }
}
