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

/// Runs the plan's script the way `o` says: what a program that combines several plans under one prompt (`doze doctor
/// --fix`) calls with the combined one. The person's parts run around root's: a removal of trust before it, a trust
/// after it ([`Plan::as_user_before`], [`Plan::as_user`]).
pub fn run(o: &mut Options<'_>, plan: &Plan) -> Result<(), Error> {
    if matches!(o.how, How::Print) {
        writeln!(o.out, "# {}", plan.what)?;
        write!(o.out, "{}", pasteable(plan))?;
        return Ok(());
    }
    let person = |o: &mut Options<'_>, part: UserPart| -> Result<(), Error> {
        let script = part.of(plan);
        if script.trim().is_empty() {
            return Ok(());
        }
        writeln!(o.out, "{}", part.asking())?;
        run_as_user(script, part).map_err(|e| failed(plan, io::Error::other(e.to_string())))
    };
    person(o, UserPart::Untrust)?;
    run_root(o, plan)?;
    person(o, UserPart::Trust)
}

/// Which of a plan's parts for the person: what macOS's dialog is for, and what a cancel leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserPart {
    /// [`Plan::as_user_before`]: trust settings removed, before root deletes the certificates.
    Untrust,
    /// [`Plan::as_user`]: the certificate trusted, after root added it to the System keychain.
    Trust,
}

impl UserPart {
    /// The part's script in `plan`.
    pub fn of(self, plan: &Plan) -> &str {
        match self {
            UserPart::Untrust => &plan.as_user_before,
            UserPart::Trust => &plan.as_user,
        }
    }

    /// What is said before macOS's dialog shows.
    pub fn asking(self) -> &'static str {
        match self {
            UserPart::Untrust => {
                "macOS asks for your password in its own dialog: removing the System trust settings of doze's \
                 certificate authority"
            }
            UserPart::Trust => {
                "macOS asks for your password in its own dialog: trusting doze's certificate authority"
            }
        }
    }
}

/// The words every cancelled dialog of macOS's starts with: the app tells this cancel from a cancelled password prompt
/// of its own by them (it ran something already, so "nothing was changed" would be false).
pub const MAC_DIALOG_CANCELLED: &str = "cancelled in macOS's dialog";

/// Why a person's part of a plan did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRefused {
    /// The person closed macOS's dialog.
    Cancelled(UserPart),
    /// macOS had no session to show its dialog in (over ssh, or with nobody logged in at the screen).
    NoDialog(UserPart),
    /// It ran and failed; what it said.
    Failed(UserPart, String),
}

impl std::fmt::Display for UserRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRefused::Cancelled(UserPart::Trust) => write!(
                f,
                "{MAC_DIALOG_CANCELLED}: doze's certificate authority is in the System keychain but not trusted yet, \
                 so https:// names are refused; run it again and allow the change"
            ),
            UserRefused::Cancelled(UserPart::Untrust) => write!(
                f,
                "{MAC_DIALOG_CANCELLED}: doze stopped before its administrator part, so no certificate was deleted \
                 and each one whose trust was not removed yet is still trusted; run it again and allow the change"
            ),
            UserRefused::NoDialog(part) => write!(
                f,
                "macOS could not show its dialog {} doze's certificate authority (no login session at the screen \
                 here): run the command in Terminal on this Mac",
                match part {
                    UserPart::Trust => "to trust",
                    UserPart::Untrust => "to stop trusting",
                }
            ),
            UserRefused::Failed(UserPart::Trust, why) => {
                write!(f, "trusting doze's certificate authority failed: {why}")
            }
            UserRefused::Failed(UserPart::Untrust, why) => write!(
                f,
                "removing the trust of doze's certificate authority failed, so nothing more was removed: {why}"
            ),
        }
    }
}

/// Runs a person's part of a plan as the person, in their login session, so macOS can show its own authorization
/// dialog: directly when this process is the person (a terminal, or the app's child); as the person who ran sudo when
/// this process is root (`launchctl asuser`, which puts the command in their session). Nothing to run is success.
pub fn run_as_user(script: &str, part: UserPart) -> Result<(), UserRefused> {
    if script.trim().is_empty() {
        return Ok(());
    }
    doze_os::guard::real_machine("macOS's trust dialog (security, as the person)");
    let script = format!("set -e\n{script}");
    let root = doze_os::privilege::is_privileged().unwrap_or(false);
    let sudo_uid = std::env::var("SUDO_UID")
        .ok()
        .filter(|u| !u.is_empty() && u.bytes().all(|b| b.is_ascii_digit()) && u != "0");
    let mut c = match (root, sudo_uid) {
        (true, Some(uid)) => {
            let mut c = Command::new("launchctl");
            c.args([
                "asuser",
                &uid,
                "sudo",
                "-u",
                &format!("#{uid}"),
                "sh",
                "-c",
                &script,
            ]);
            c
        }
        _ => {
            let mut c = Command::new("sh");
            c.args(["-c", &script]);
            c
        }
    };
    let out = c
        .stdin(Stdio::null())
        .output()
        .map_err(|e| UserRefused::Failed(part, e.to_string()))?;
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !said.trim().is_empty() {
        eprint!("{said}");
    }
    user_outcome(out.status.success(), &said, part)
}

/// What a person's part came to, from its exit and what `security` said: Security framework's own words for a
/// cancelled dialog (errAuthorizationCanceled) and for one it could not show (errAuthorizationInteractionNotAllowed).
pub fn user_outcome(ok: bool, said: &str, part: UserPart) -> Result<(), UserRefused> {
    if ok {
        return Ok(());
    }
    let low = said.to_ascii_lowercase();
    if low.contains("canceled by the user") || low.contains("cancelled by the user") {
        return Err(UserRefused::Cancelled(part));
    }
    if low.contains("no user interaction was possible") {
        return Err(UserRefused::NoDialog(part));
    }
    let last = said
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("it exited without saying why");
    Err(UserRefused::Failed(part, last.to_owned()))
}

/// The plan's root part, unless it has none (all that is left is the person's part: no password prompt for nothing).
fn run_root(o: &mut Options<'_>, plan: &Plan) -> Result<(), Error> {
    if plan.script.trim().is_empty() {
        return Ok(());
    }
    // Already privileged: run it, and do not ask for a program that is probably not there. This is the container
    // case, and it used to fail twice over: sudo is absent from most base images (alpine, debian:stable-slim), so a
    // process running as root with every permission it needs shelled out to a missing binary and reported that the
    // privileged step failed, while saying "doze needs sudo once", the opposite of true.
    let privileged = doze_os::privilege::is_privileged().unwrap_or(false);
    if privileged || !matches!(o.how, How::With(_) | How::Print) {
        doze_os::guard::real_machine("a privileged setup script");
    }
    if privileged {
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
///
/// The person's parts ([`Plan::as_user_before`], [`Plan::as_user`]) come before and after, without sudo and said to be so:
/// under sudo, macOS refuses the trust change wherever it cannot show its dialog. The parts are one command line joined
/// by `&&` (their heredocs' bodies follow it, in order), so a part that stops, a cancelled dialog above all, stops
/// what comes after it: a removal's root part never deletes a certificate whose trust is still set.
pub fn pasteable(plan: &Plan) -> String {
    match plan.shell {
        Shell::Sh => {
            const DELIM: &str = "DOZE_SETUP";
            const USER_DELIM: &str = "DOZE_AS_YOU";
            // (note, command, delimiter, body); a person's part with `set -e`: a cancelled dialog stops it, as it does
            // when doze runs it.
            let mut parts: Vec<(&str, &str, &str, String)> = Vec::new();
            let person = |s: &str| format!("set -e\n{}", s.trim_end_matches('\n'));
            if !plan.as_user_before.trim().is_empty() {
                parts.push((
                    AS_YOU_FIRST_NOTE,
                    "sh",
                    USER_DELIM,
                    person(&plan.as_user_before),
                ));
            }
            if !plan.script.trim().is_empty() {
                let body = plan.script.trim_end_matches('\n').to_owned();
                parts.push(("", "sudo sh", DELIM, body));
            }
            if !plan.as_user.trim().is_empty() {
                parts.push((AS_YOU_NOTE, "sh", USER_DELIM, person(&plan.as_user)));
            }
            let mut out = String::new();
            for (note, ..) in &parts {
                if !note.is_empty() {
                    out.push_str(note);
                    out.push('\n');
                }
            }
            let line: Vec<String> = parts
                .iter()
                .map(|(_, cmd, delim, _)| format!("{cmd} <<'{delim}'"))
                .collect();
            if !line.is_empty() {
                out.push_str(&line.join(" && "));
                out.push('\n');
            }
            for (_, _, delim, body) in &parts {
                out.push_str(&format!("{body}\n{delim}\n"));
            }
            out
        }
        Shell::PowerShell => format!(
            "# in a PowerShell run as administrator:\n{}\n",
            plan.script.trim_end_matches('\n')
        ),
    }
}

/// The line before the person's part after root's (a trust) in a pasteable script (the app looks for "as you, not
/// under sudo" to say that macOS asks too).
pub const AS_YOU_NOTE: &str =
    "# then as you, not under sudo: macOS asks for your password in its own dialog";

/// The line before the person's part before root's (a removal of trust) in a pasteable script.
pub const AS_YOU_FIRST_NOTE: &str =
    "# first as you, not under sudo: macOS asks for your password in its own dialog";

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
