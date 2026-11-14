//! The machine's own hands: the `doze` CLI run as the person with the system's prompt in place of sudo, the
//! administrator prompt itself (`doze_os::privilege::admin`), processes by exact pid, folders.
//!
//! What a command prints is said line by line as it comes ([`Said`], through [`SystemHands::out`]), so the app can
//! stream it into its sheet. A script run as root through the system's prompt prints nothing until it ends (the
//! prompt's `do shell script` keeps the output); so the script's output goes to a file of the person's own, which is
//! read as it grows.

use std::io::{BufRead as _, BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::cli::Cli;
use super::repair::{CANCELLED_TEXT, Fail, Hands, doze_outcome};

/// A line of what is being done, as it happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// A command starts: `doze dns-setup`, `kill -TERM 4182`, or `as root: remove stale routes`.
    Run(String),
    /// A line it printed.
    Line(String),
    /// A note from the app: "waiting for the password…".
    Note(String),
}

/// Where the lines go.
pub type Out = Arc<dyn Fn(Said) + Send + Sync>;

/// The real machine. Only the app makes one, and only on a person's click.
#[derive(Clone)]
pub struct SystemHands {
    /// The `doze` to run (the command source).
    pub cli: Cli,
    /// Where a doze command with no stack runs.
    pub home: PathBuf,
    /// Where what is done is said, line by line.
    pub out: Option<Out>,
    /// Runs a root script in place of the system's prompt: `RUNNER <sentence> <script>`. For a test's stand-in only;
    /// the app sets it in a development build alone.
    pub admin_runner: Option<PathBuf>,
}

impl std::fmt::Debug for SystemHands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemHands")
            .field("cli", &self.cli)
            .field("home", &self.home)
            .field("admin_runner", &self.admin_runner)
            .finish_non_exhaustive()
    }
}

impl SystemHands {
    pub fn new(cli: Cli, home: PathBuf) -> SystemHands {
        SystemHands {
            cli,
            home,
            out: None,
            admin_runner: None,
        }
    }

    fn say(&self, s: Said) {
        if let Some(out) = &self.out {
            out(s);
        }
    }

    /// Runs `cmd`, saying each line it prints as it comes; its exit and everything it printed.
    fn stream(&self, mut cmd: Command) -> std::io::Result<(bool, String)> {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let all = Arc::new(Mutex::new(String::new()));
        let pipe = |r: Box<dyn std::io::Read + Send>| {
            let (all, out) = (all.clone(), self.out.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(r).split(b'\n').map_while(Result::ok) {
                    let line = text_of(&line);
                    if let Ok(mut a) = all.lock() {
                        a.push_str(&line);
                        a.push('\n');
                    }
                    if let Some(out) = &out {
                        out(Said::Line(line));
                    }
                }
            })
        };
        let mut readers = Vec::new();
        if let Some(o) = child.stdout.take() {
            readers.push(pipe(Box::new(o)));
        }
        if let Some(e) = child.stderr.take() {
            readers.push(pipe(Box::new(e)));
        }
        for r in readers {
            let _ = r.join();
        }
        let ok = child.wait()?.success();
        let text = all.lock().map(|a| a.clone()).unwrap_or_default();
        Ok((ok, text))
    }

    /// The stand-in runner, run as the person, its output said as it comes.
    fn admin_by_runner(&self, runner: &Path, what: &str, script: &str) -> Result<(), Fail> {
        let mut c = Command::new(runner);
        c.arg(doze_os::privilege::prompt(what)).arg(script);
        let (ok, text) = self
            .stream(c)
            .map_err(|e| Fail::Failed(format!("could not run {}: {e}", runner.display())))?;
        if ok {
            Ok(())
        } else if text.contains("(-128)") || text.contains(CANCELLED_TEXT) {
            Err(Fail::Cancelled)
        } else {
            Err(Fail::Failed(last_line(
                &text,
                "the administrator prompt failed",
            )))
        }
    }

    /// The system's prompt, the script's output read from a file of the person's own as it grows.
    fn admin_by_prompt(&self, what: &str, script: &str) -> Result<(), Fail> {
        let log = OutputFile::new();
        let wrapped = match &log {
            Some(l) => format!(
                "exec >>{} 2>&1\n{script}",
                super::quote(&l.path.to_string_lossy())
            ),
            None => script.to_owned(),
        };
        let (what, done) = (what.to_owned(), Arc::new(Mutex::new(None)));
        let worker = {
            let done = done.clone();
            std::thread::spawn(move || {
                let r = doze_os::privilege::admin(&what, &wrapped);
                if let Ok(mut d) = done.lock() {
                    *d = Some(r);
                }
            })
        };
        let mut log = log;
        loop {
            let finished = done.lock().map(|d| d.is_some()).unwrap_or(true);
            if let Some(l) = log.as_mut() {
                for line in l.read_new(finished) {
                    self.say(Said::Line(line));
                }
            }
            if finished {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = worker.join();
        let r = done.lock().ok().and_then(|mut d| d.take());
        match r {
            Some(Ok(())) => Ok(()),
            Some(Err(doze_os::privilege::AdminError::Cancelled)) => Err(Fail::Cancelled),
            Some(Err(e)) => Err(Fail::Failed(e.to_string())),
            None => Err(Fail::Failed(
                "the administrator prompt did not answer".into(),
            )),
        }
    }
}

/// A line's bytes as text: its end (`\n`, `\r\n`) dropped, anything not UTF-8 replaced (a tool may print Latin-1).
fn text_of(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8_lossy(line).into_owned()
}

/// The last line of `text` that says something, else `none`.
fn last_line(text: &str, none: &str) -> String {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(none)
        .to_owned()
}

/// A file of the person's own that a root script appends its output to, read as it grows; deleted after.
struct OutputFile {
    path: PathBuf,
    read: usize,
    /// Bytes of a line not ended yet (kept as bytes: a character may be split across two reads).
    partial: Vec<u8>,
}

impl OutputFile {
    fn new() -> Option<OutputFile> {
        let dir = std::env::temp_dir();
        for n in 0..100u32 {
            let path = dir.join(format!("doze-app-admin-{}-{n}.log", std::process::id()));
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            if o.open(&path).is_ok() {
                return Some(OutputFile {
                    path,
                    read: 0,
                    partial: Vec::new(),
                });
            }
        }
        None
    }

    /// The whole lines added since the last read; the rest too once `last`.
    fn read_new(&mut self, last: bool) -> Vec<String> {
        let mut buf = Vec::new();
        if let Ok(mut f) = std::fs::File::open(&self.path) {
            let _ = f.read_to_end(&mut buf);
        }
        if let Some(new) = buf.get(self.read..) {
            self.partial.extend_from_slice(new);
            self.read = buf.len();
        }
        let mut out: Vec<String> = Vec::new();
        while let Some(i) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=i).collect();
            out.push(text_of(&line));
        }
        if last && !self.partial.is_empty() {
            out.push(text_of(&std::mem::take(&mut self.partial)));
        }
        out
    }
}

impl Drop for OutputFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Hands for SystemHands {
    fn doze(&mut self, dir: Option<&Path>, args: &[String]) -> Result<(), Fail> {
        let at = dir.unwrap_or(&self.home);
        self.say(Said::Run(self.cli.line(args)));
        let mut c = Command::new(&self.cli.program);
        c.args(args)
            .current_dir(at)
            .envs(self.cli.env())
            .env_remove("DOZE_APP_CHILD");
        let (ok, text) = self
            .stream(c)
            .map_err(|e| Fail::Failed(format!("could not run doze: {e}")))?;
        doze_outcome(ok, &text)
    }

    fn admin(&mut self, what: &str, script: &str) -> Result<(), Fail> {
        self.say(Said::Run(format!("as root: {what}")));
        self.say(Said::Note(
            "waiting for the administrator's password…".into(),
        ));
        match self.admin_runner.clone() {
            Some(r) => self.admin_by_runner(&r, what, script),
            None => self.admin_by_prompt(what, script),
        }
    }

    fn terminate(&mut self, pid: u32) -> Result<(), Fail> {
        self.say(Said::Run(format!("kill -TERM {pid}")));
        doze_os::process::terminate(pid).map_err(|e| Fail::Failed(e.to_string()))
    }

    fn kill(&mut self, pid: u32) -> Result<(), Fail> {
        self.say(Said::Run(format!("kill -KILL {pid}")));
        doze_os::process::kill(pid).map_err(|e| Fail::Failed(e.to_string()))
    }

    fn alive(&mut self, pid: u32) -> bool {
        doze_os::process::alive(pid)
    }

    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(100));
    }

    fn remove_dir(&mut self, dir: &Path) -> Result<(), Fail> {
        self.say(Said::Run(format!(
            "rm -r {}",
            super::quote(&dir.to_string_lossy())
        )));
        std::fs::remove_dir_all(dir).map_err(|e| Fail::Failed(e.to_string()))
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::panic)] // WHY: the helpers that write the stand-in scripts are test code
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{Said, SystemHands};
    use crate::settings::cli::Cli;
    use crate::settings::repair::{Fail, Hands};

    /// A script standing in for doze or the prompt: it prints, then exits as told.
    fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
        p
    }

    fn hands(
        cli: std::path::PathBuf,
        home: &std::path::Path,
    ) -> (SystemHands, Arc<Mutex<Vec<Said>>>) {
        let said = Arc::new(Mutex::new(Vec::new()));
        let mut h = SystemHands::new(Cli::go(cli), home.to_path_buf());
        let s = said.clone();
        h.out = Some(Arc::new(move |l| {
            s.lock().unwrap_or_else(|e| e.into_inner()).push(l)
        }));
        (h, said)
    }

    #[test]
    fn a_command_streams_its_lines_and_its_environment() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let doze = script(
            dir.path(),
            "doze",
            "echo \"one $1 prompt=$DOZE_ADMIN_PROMPT\"; echo two >&2",
        );
        let (mut h, said) = hands(doze, dir.path());
        assert_eq!(h.doze(None, &["dns-setup".into()]), Ok(()));
        let said = said.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(said.first(), Some(&Said::Run("doze dns-setup".into())));
        assert!(said.contains(&Said::Line("one dns-setup prompt=1".into())));
        assert!(said.contains(&Said::Line("two".into())));
    }

    #[test]
    fn a_failure_says_its_last_line_and_a_cancel_is_a_cancel() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let doze = script(dir.path(), "doze", "echo 'it broke' >&2; exit 1");
        let (mut h, _) = hands(doze, dir.path());
        assert_eq!(
            h.doze(None, &["trust".into()]),
            Err(Fail::Failed("it broke".into()))
        );
        let doze = script(
            dir.path(),
            "doze2",
            "echo 'the administrator prompt was cancelled' >&2; exit 1",
        );
        let (mut h, _) = hands(doze, dir.path());
        assert_eq!(h.doze(None, &["trust".into()]), Err(Fail::Cancelled));
    }

    #[test]
    fn the_stand_in_runner_gets_the_sentence_and_the_script() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let runner = script(
            dir.path(),
            "runner",
            "echo \"asked: $1\"; echo \"would run: $2\"",
        );
        let (mut h, said) = hands(dir.path().join("doze"), dir.path());
        h.admin_runner = Some(runner);
        assert_eq!(
            h.admin("remove stale routes", "rm -f /etc/resolver/x"),
            Ok(())
        );
        let said = said.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert!(said.contains(&Said::Line(
            "asked: doze needs an administrator's password to remove stale routes.".into()
        )));
        assert!(said.contains(&Said::Line("would run: rm -f /etc/resolver/x".into())));
        let cancel = script(
            dir.path(),
            "cancel",
            "echo 'User canceled. (-128)' >&2; exit 1",
        );
        h.admin_runner = Some(cancel);
        assert_eq!(h.admin("x", "true"), Err(Fail::Cancelled));
    }

    #[test]
    fn an_output_file_reads_whole_lines_as_they_come() {
        let mut f = super::OutputFile::new().unwrap_or_else(|| panic!("no output file"));
        std::fs::write(&f.path, "a\nb").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.read_new(false), ["a"]);
        std::fs::write(&f.path, "a\nbc\nd").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.read_new(true), ["bc", "d"]);
        // A tick split across two reads stays one character.
        let tick = "✓ ok\n".as_bytes();
        let mut g = super::OutputFile::new().unwrap_or_else(|| panic!("no output file"));
        std::fs::write(&g.path, &tick[..2]).unwrap_or_else(|e| panic!("{e}"));
        assert!(g.read_new(false).is_empty());
        std::fs::write(&g.path, tick).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(g.read_new(false), ["✓ ok"]);
        let path = f.path.clone();
        drop(f);
        assert!(!path.exists());
    }
}
