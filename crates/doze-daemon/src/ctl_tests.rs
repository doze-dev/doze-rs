//! ctl's tests: the blocks record, and how a start judges the daemon it spawned.

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod blocks_tests {
    use crate::ctl::*;

    /// The record reads back as written, and Go's file (the same shape, Go's prints) reads too.
    #[test]
    fn the_blocks_record_reads_back_and_reads_gos() {
        let dir = std::env::temp_dir().join(format!("doze-blocks-{}", std::process::id()));
        let run = RunDir::of(&dir);
        std::fs::create_dir_all(run.path()).unwrap();
        let src = "app \"api\" {\n  run = \"sleep 600\"\n}\n";
        std::fs::write(dir.join("doze.hcl"), src).unwrap();
        let path = dir.join("doze.hcl").to_string_lossy().into_owned();
        let cfg =
            crate::entry::with_options("0.0.0", &doze_config::NoModules, Default::default(), |o| {
                doze_config::load_shape(&path, o)
            })
            .unwrap();
        write_blocks(&run, &cfg).unwrap();
        assert_eq!(blocks(&run).unwrap(), cfg.block_fingerprints());
        std::fs::write(
            run.blocks_file(),
            r#"{"db": {"kind": "module", "type": "postgres", "fp": "ab12"}}"#,
        )
        .unwrap();
        let go = blocks(&run).unwrap();
        assert_eq!(go["db"].fingerprint, "ab12");
        std::fs::write(run.blocks_file(), "[]").unwrap();
        assert_eq!(blocks(&run), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::panic)]
mod start_tests {
    use crate::ctl::*;

    /// A start's daemon is judged by its own exit status: one still running is, one that ended says how (here killed
    /// outright, as a daemon that wrote "serving" and then nothing was on a loaded machine), whatever its pid is now.
    #[test]
    fn a_daemon_that_ended_says_how_and_one_running_does_not() {
        let mut up = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap();
        assert_eq!(still_running(&mut up), Ok(()));
        let _ = up.kill();
        let _ = up.wait();
        let mut killed = std::process::Command::new("/bin/sh")
            .args(["-c", "kill -KILL $$"])
            .spawn()
            .unwrap();
        let t = Instant::now();
        let how = loop {
            match still_running(&mut killed) {
                Err(how) => break how,
                Ok(()) if t.elapsed() < Duration::from_secs(30) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(()) => panic!("still running"),
            }
        };
        assert_eq!(how, "signal: killed");
        let mut failed = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 3"])
            .spawn()
            .unwrap();
        let _ = failed.wait();
        assert_eq!(still_running(&mut failed), Err("exit status 3".into()));
    }
}
