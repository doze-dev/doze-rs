//! Where an instance's processes print: one file per instance, appended to, never a pipe (W0: a tool that leaves a
//! server behind keeps captured pipes open forever).
//!
//! This is E1's log: `run/<name>.log` under the stack's data folder. E2 replaces it with Go's log store (runs as
//! segments under `logs/`), behind the same three calls.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// One instance's log.
#[derive(Debug, Clone)]
pub struct RunLog {
    path: PathBuf,
}

impl RunLog {
    /// A new run of `name`'s log in `dir`: the previous run moves to `<name>.prev.log`, so a `log_line` check and a
    /// failure's quote read only this start's lines (Go reads the newest run only).
    pub fn open(dir: &Path, name: &str) -> io::Result<RunLog> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{name}.log"));
        if path.exists() {
            std::fs::rename(&path, dir.join(format!("{name}.prev.log")))?;
        }
        append(&path)?;
        Ok(RunLog { path })
    }

    /// The file, for a `log_line` check and for tests.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A handle a child writes its output to.
    pub fn stdio(&self) -> io::Result<Stdio> {
        append(&self.path).map(Stdio::from)
    }

    /// Adds one line doze writes itself (a step's or a hook's), tagged.
    pub fn line(&self, tag: &str, text: &str) {
        if let Ok(mut f) = append(&self.path) {
            let _ = writeln!(f, "[{tag}] {text}");
        }
    }

    /// The last `n` lines.
    pub fn tail(&self, n: usize) -> Vec<String> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let from = lines.len().saturating_sub(n);
        lines.iter().skip(from).map(|l| (*l).to_string()).collect()
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}
