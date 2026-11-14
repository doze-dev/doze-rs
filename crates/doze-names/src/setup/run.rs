//! Running the one privileged script: printed, run as is by a process that is already privileged, handed to the
//! caller's own runner, or run under one prompt (sudo on a terminal; the system's administrator prompt where there is
//! none). One prompt matters: a setup that asks three times trains people to type their password without reading what
//! it is for.

use super::{Plan, Shell};
use crate::Error;
use std::io::{self, Write};
use std::process::{Command, Stdio};

/// How the script is run.
pub enum How<'a> {
    /// Written to `out` instead, for anyone who would rather paste it themselves than hand a tool their password.
    Print,
    /// Under `sudo` on this terminal, its output streaming; on Windows, which has no sudo, as [`How::Prompt`].
    Terminal,
    /// Under the system's administrator prompt, for a program with no terminal: the macOS password dialog
    /// (osascript), polkit's (pkexec), or UAC. Through `privesc` (PLAN.md, "Library spikes").
    Prompt,
    /// Handed to the caller, who asks for the password its own way.
    With(&'a mut dyn FnMut(&Plan) -> io::Result<()>),
}

impl std::fmt::Debug for How<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            How::Print => "Print",
            How::Terminal => "Terminal",
            How::Prompt => "Prompt",
            How::With(_) => "With",
        })
    }
}

/// Where progress goes, and how the script runs.
pub struct Options<'a> {
    pub out: &'a mut dyn Write,
    pub how: How<'a>,
}

impl std::fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options").field("how", &self.how).finish()
    }
}

fn failed(plan: &Plan, why: io::Error) -> Error {
    Error::Privileged {
        what: plan.what.clone(),
        why,
    }
}

/// Runs the plan's script the way `o` says.
pub(crate) fn run(o: &mut Options<'_>, plan: &Plan) -> Result<(), Error> {
    if matches!(o.how, How::Print) {
        writeln!(o.out, "# {}", plan.what)?;
        write!(o.out, "{}", pasteable(plan))?;
        return Ok(());
    }
    // Already privileged: run it, and do not ask for a program that is probably not there. This is the container
    // case, and it used to fail twice over: sudo is absent from most base images (alpine, debian:stable-slim), so a
    // process running as root with every permission it needs shelled out to a missing binary and reported that the
    // privileged step failed, while saying "doze needs sudo once", the opposite of true.
    if doze_os::privilege::is_privileged().unwrap_or(false) {
        return finish(plan, direct(plan).status());
    }
    match &mut o.how {
        How::With(runner) => runner(plan).map_err(|e| failed(plan, e)),
        How::Terminal if plan.shell == Shell::Sh => {
            writeln!(o.out, "doze needs sudo once: {}", plan.what)?;
            let status = Command::new("sudo")
                .args(["sh", "-c", &plan.script])
                .stdin(Stdio::inherit())
                .status();
            finish(plan, status)
        }
        How::Terminal | How::Prompt => prompt(plan),
        How::Print => Ok(()),
    }
}

/// The script run by this process's own shell, for a process that is privileged already.
fn direct(plan: &Plan) -> Command {
    match plan.shell {
        Shell::Sh => {
            let mut c = Command::new("sh");
            c.args(["-c", &plan.script]);
            c
        }
        Shell::PowerShell => {
            let mut c = Command::new("powershell");
            c.args(powershell_args(&plan.script));
            c
        }
    }
}

/// The arguments that run a PowerShell script exactly as written: base64 of its UTF-16, so no quote in it meets a
/// command line's quoting rules (PowerShell 5.1 strips embedded double quotes from `-Command`). Progress records are
/// turned off first: with its output redirected, PowerShell writes them as CLIXML ("Preparing modules for first use")
/// into the person's terminal (seen on the Windows runner).
pub(crate) fn powershell_args(script: &str) -> Vec<String> {
    let script = format!("{QUIET}{script}");
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-EncodedCommand".into(),
        base64(&utf16),
    ]
}

const QUIET: &str = "$ProgressPreference = 'SilentlyContinue'\n";

/// Under the system's own administrator prompt.
fn prompt(plan: &Plan) -> Result<(), Error> {
    let (program, args) = match plan.shell {
        Shell::Sh => (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), plan.script.clone()],
        ),
        Shell::PowerShell => ("powershell.exe".to_string(), powershell_args(&plan.script)),
    };
    let out = privesc::PrivilegedCommand::new(program)
        .args(args)
        .gui(true)
        .prompt(format!(
            "doze needs administrator rights once: {}",
            plan.what
        ))
        .run()
        .map_err(|e| failed(plan, io::Error::other(e.to_string())))?;
    if out.success() {
        return Ok(());
    }
    let said = out.stderr_str().unwrap_or_default().trim().to_string();
    Err(failed(
        plan,
        io::Error::other(format!("the script failed ({}): {said}", out.status)),
    ))
}

fn finish(plan: &Plan, status: io::Result<std::process::ExitStatus>) -> Result<(), Error> {
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            eprintln!(
                "\nthe privileged step failed. To do it by hand:\n\n{}",
                pasteable(plan)
            );
            Err(failed(
                plan,
                io::Error::other(format!("the script exited with {s}")),
            ))
        }
        Err(e) => Err(failed(plan, e)),
    }
}

/// The script as something a person can paste.
///
/// Not `sudo sh -c '<script>'`: the script has single quotes of its own (heredoc delimiters, printf arguments) and the
/// first of them ends the quoting, so the pasted command breaks in a way that is tedious to diagnose. Feeding it on
/// stdin through a quoted heredoc sidesteps quoting entirely, and nests with the heredocs inside the script because the
/// delimiters differ. A PowerShell script is pasted into an elevated PowerShell as it is.
pub fn pasteable(plan: &Plan) -> String {
    match plan.shell {
        Shell::Sh => {
            const DELIM: &str = "DOZE_SETUP";
            format!(
                "sudo sh <<'{DELIM}'\n{}\n{DELIM}\n",
                plan.script.trim_end_matches('\n')
            )
        }
        Shell::PowerShell => format!(
            "# in a PowerShell run as administrator:\n{}\n",
            plan.script.trim_end_matches('\n')
        ),
    }
}

/// `s` as one single-quoted shell word. The script is assembled from parts and handed to a shell, so quoting is not
/// optional even where the parts are our own constants.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `s` as one single-quoted PowerShell string (a quote inside is doubled).
pub(crate) fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Standard base64 with padding: a dozen lines, so no dependency for one use.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (b(0) << 16) | (b(1) << 8) | b(2);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            let c = ABC
                .get(((n >> shift) & 63) as usize)
                .copied()
                .unwrap_or(b'=');
            out.push(if i <= chunk.len() { char::from(c) } else { '=' });
        }
    }
    out
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
