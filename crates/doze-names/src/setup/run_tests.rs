use super::*;

fn sh(what: &str, script: &str) -> Plan {
    Plan::root(what, Shell::Sh, script)
}

/// Go: TestShellQuote. The script is assembled by concatenation and handed to a shell, so a quote in any value must not
/// be able to end the argument.
#[test]
fn shell_quote_cannot_be_ended_early() {
    for s in ["plain", "with 'quotes'", "$(whoami)", "a\nb"] {
        let q = shell_quote(s);
        assert!(q.starts_with('\'') && q.ends_with('\''), "{q}");
    }
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
    assert_eq!(ps_quote("it's"), "'it''s'");
}

/// Go: TestPasteableScriptSurvivesItsOwnQuotes. The scripts contain single quotes (heredoc delimiters and printf
/// arguments), so wrapping them in `sudo sh -c '…'` breaks at the first one; anyone cautious enough to read the script
/// before running it is exactly the person who would hit that.
#[test]
fn the_pasteable_script_survives_its_own_quotes() {
    let script =
        "cat > /tmp/x <<'PLIST'\n<plist/>\nPLIST\nprintf 'nameserver 127.0.0.1\\n' > /tmp/y\n";
    let got = pasteable(&sh("x", script));
    assert!(
        !got.contains("sh -c"),
        "sh -c cannot carry a script with quotes"
    );
    assert!(got.starts_with("sudo sh <<'DOZE_SETUP'\n"), "{got}");
    assert!(got.ends_with("\nDOZE_SETUP\n"), "{got}");
    assert!(got.contains("<<'PLIST'"), "the inner heredoc is kept");
    let body = got
        .trim_start_matches("sudo sh <<'DOZE_SETUP'\n")
        .trim_end_matches("\nDOZE_SETUP\n");
    assert!(
        !body.contains("DOZE_SETUP"),
        "the delimiter is not in the body"
    );
}

/// Go: TestRunReplacesSudo. A program with no terminal hands the script to its own runner, once, with what it is for;
/// nothing is printed and sudo is not asked. Its error comes back, still matchable; Print prints and runs nothing.
#[test]
fn a_runner_replaces_sudo() {
    if doze_os::privilege::is_privileged().unwrap_or(false) {
        return; // root runs the script directly
    }
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let mut runner = |p: &Plan| -> io::Result<()> {
        seen.push((p.what.clone(), p.script.clone()));
        Ok(())
    };
    let mut o = Options {
        out: &mut out,
        how: How::With(&mut runner),
    };
    run(&mut o, &sh("route .doze", "echo hi")).unwrap();
    assert_eq!(seen, [("route .doze".to_string(), "echo hi".to_string())]);
    assert!(out.is_empty(), "the runner speaks for itself");

    let mut refuse = |_: &Plan| -> io::Result<()> { Err(io::Error::other("cancelled")) };
    let mut o = Options {
        out: &mut out,
        how: How::With(&mut refuse),
    };
    match run(&mut o, &sh("x", "y")) {
        Err(Error::Privileged { what, why }) => {
            assert_eq!(
                (what.as_str(), why.to_string().as_str()),
                ("x", "cancelled")
            );
        }
        other => panic!("want the runner's error, got {other:?}"),
    }

    let mut o = Options {
        out: &mut out,
        how: How::Print,
    };
    run(&mut o, &sh("x", "y")).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "# x\nsudo sh <<'DOZE_SETUP'\ny\nDOZE_SETUP\n"
    );
}

/// PowerShell gets its script as base64 of UTF-16LE (RFC 4648 vectors, and the encoding PowerShell decodes).
#[test]
fn powershell_gets_the_script_encoded_whole() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    assert_eq!(
        base64(&[
            0x65, 0, 0x63, 0, 0x68, 0, 0x6f, 0, 0x20, 0, 0x27, 0, 0xe9, 0, 0x27, 0
        ]),
        "ZQBjAGgAbwAgACcA6QAnAA=="
    );
    let args = powershell_args("echo 'é'");
    let whole: Vec<u8> = format!("{QUIET}echo 'é'")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(
        args.last().unwrap(),
        &base64(&whole),
        "progress off, then the script"
    );
}

/// The person's parts are pasted around root's, not under sudo, each said so and each stopping at its first failure:
/// a removal of trust before root's, a trust after. One with only a person's part asks for no sudo at all.
#[test]
fn the_persons_parts_are_pasted_around_roots_without_sudo() {
    let mut p = sh("x", "root-line\n");
    p.as_user = "user-line\n".into();
    p.as_user_before = "first-line\n".into();
    assert_eq!(
        pasteable(&p),
        format!(
            "{AS_YOU_FIRST_NOTE}\n{AS_YOU_NOTE}\n\
             sh <<'DOZE_AS_YOU' && sudo sh <<'DOZE_SETUP' && sh <<'DOZE_AS_YOU'\n\
             set -e\nfirst-line\nDOZE_AS_YOU\n\
             root-line\nDOZE_SETUP\n\
             set -e\nuser-line\nDOZE_AS_YOU\n"
        )
    );
    #[cfg(unix)]
    pasted_parts_stop_at_the_first_that_fails(&p);
    p.script.clear();
    p.as_user_before.clear();
    let only = pasteable(&p);
    assert!(!only.contains("sudo sh"), "{only}");
    assert!(only.contains("user-line"), "{only}");
    assert_eq!(
        run_as_user("", UserPart::Trust),
        Ok(()),
        "no person's part: nothing runs"
    );
}

/// The pasted script as a shell runs it, with a stand-in `sudo` that runs its `sh` as the person: every part runs in
/// order when each succeeds, and a person's part that fails first (a cancelled dialog) stops root's and the rest.
#[cfg(unix)]
fn pasted_parts_stop_at_the_first_that_fails(p: &Plan) {
    let dir = std::env::temp_dir().join(format!("doze-fake-sudo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let fake = dir.join("sudo");
    std::fs::write(&fake, "#!/bin/sh\nexec \"$@\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let paste = |first: &str| {
        let mut q = p.clone();
        let say = |what: &str| {
            format!(
                "echo {what} >> {}\n",
                shell_quote(&log.display().to_string())
            )
        };
        q.as_user_before = format!("{}{first}\n", say("first"));
        q.script = say("root");
        q.as_user = say("after");
        let _ = std::fs::remove_file(&log);
        let out = Command::new("sh")
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .arg("-c")
            .arg(pasteable(&q))
            .output()
            .unwrap();
        (
            out.status.success(),
            std::fs::read_to_string(&log).unwrap_or_default(),
        )
    };
    assert_eq!(paste("true"), (true, "first\nroot\nafter\n".into()));
    assert_eq!(
        paste("exit 1"),
        (false, "first\n".into()),
        "a stopped first part stops root's part and the rest"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What `security` says when macOS's dialog is cancelled, or cannot be shown, is told apart from a failure, and each
/// says what stands: a cancelled trust leaves the certificate in the keychain untrusted, a cancelled removal leaves
/// the authority trusted. Both start with the words the app looks for.
#[test]
fn the_persons_part_tells_a_cancel_from_a_missing_dialog() {
    let t = UserPart::Trust;
    assert_eq!(user_outcome(true, "anything", t), Ok(()));
    assert_eq!(
        user_outcome(
            false,
            "SecTrustSettingsSetTrustSettings: The authorization was canceled by the user.\n",
            t
        ),
        Err(UserRefused::Cancelled(t))
    );
    assert_eq!(
        user_outcome(
            false,
            "SecTrustSettingsSetTrustSettings: The authorization was denied since no user interaction was possible.\n",
            t
        ),
        Err(UserRefused::NoDialog(t))
    );
    assert_eq!(
        user_outcome(false, "first\nError reading file x\n\n", t),
        Err(UserRefused::Failed(t, "Error reading file x".into()))
    );
    let trust = UserRefused::Cancelled(UserPart::Trust).to_string();
    assert!(
        trust.starts_with(MAC_DIALOG_CANCELLED)
            && trust.contains("in the System keychain but not trusted yet"),
        "{trust}"
    );
    let untrust = UserRefused::Cancelled(UserPart::Untrust).to_string();
    assert!(
        untrust.starts_with(MAC_DIALOG_CANCELLED) && untrust.contains("still trusted"),
        "{untrust}"
    );
}

/// A stand-in `security` on the PATH of a person's part, run as doze runs it (`set -e`): it logs what it was asked,
/// trusts every root (verify-cert passes), and answers a trust change as `mode` says.
#[cfg(unix)]
fn person_run(script: &str, part: UserPart, mode: &str) -> (Result<(), UserRefused>, String) {
    let dir = std::env::temp_dir().join(format!(
        "doze-fake-security-{}-{mode}-{}",
        std::process::id(),
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace(':', "_")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("log");
    let fake = dir.join("security");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{log}'\ncase \"$1\" in\n  verify-cert) exit 0 ;;\nesac\ncase '{mode}' in\n  \
             cancel) echo 'SecTrustSettingsSetTrustSettings: The authorization was canceled by the user.' >&2; exit 1 ;;\n  \
             notfound) echo 'SecTrustSettingsRemoveTrustSettings: The specified item could not be found in the keychain.' >&2; exit 1 ;;\nesac\nexit 0\n",
            log = log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = Command::new("sh")
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
        .arg("-c")
        .arg(format!("set -e\n{script}"))
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let asked = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    (user_outcome(out.status.success(), &said, part), asked)
}

/// The macOS scripts as they run: a cancelled removal stops before anything else (and its plan removes trust before
/// root deletes the certificate); "not found" alone is let pass; a cancelled trust is said; a root whose bytes changed
/// since the plan is not trusted.
#[cfg(unix)]
#[test]
fn the_persons_part_stops_where_it_must() {
    use super::super::trust::{TrustRequest, https_plan, untrust_plan};
    use crate::ca::{Ca, ca_dir};
    use crate::setup::Os;
    let home = std::env::temp_dir().join(format!("doze-names-person-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let ca = Ca::load_or_create(&ca_dir(&home)).unwrap();
    let req = TrustRequest {
        ca: Ca::probe(&ca_dir(&home)).unwrap(),
        ..TrustRequest::default()
    };
    let rm = untrust_plan(Os::Mac, &req).unwrap();
    assert!(
        rm.as_user_before.contains("remove-trusted-cert -d"),
        "{rm:?}"
    );
    assert!(
        !rm.as_user_before.contains("|| true") && !rm.as_user_before.contains("2>/dev/null ||"),
        "a cancel is never swallowed: {}",
        rm.as_user_before
    );
    assert!(
        rm.as_user.is_empty() && rm.script.contains("delete-certificate"),
        "{rm:?}"
    );
    let paste = pasteable(&rm);
    assert!(
        paste.find("remove-trusted-cert") < paste.find("delete-certificate"),
        "trust goes before the certificate: {paste}"
    );
    let (got, asked) = person_run(&rm.as_user_before, UserPart::Untrust, "cancel");
    assert_eq!(got, Err(UserRefused::Cancelled(UserPart::Untrust)));
    assert_eq!(asked, "verify-cert\nremove-trusted-cert\n");
    let (got, _) = person_run(&rm.as_user_before, UserPart::Untrust, "notfound");
    assert_eq!(got, Ok(()), "a trust already gone is not a failure");

    let add = https_plan(Os::Mac, &home, Some(&ca)).unwrap();
    let (got, asked) = person_run(&add.as_user, UserPart::Trust, "cancel");
    assert_eq!(got, Err(UserRefused::Cancelled(UserPart::Trust)));
    assert_eq!(asked, "add-trusted-cert\n");
    let (got, _) = person_run(&add.as_user, UserPart::Trust, "ok");
    assert_eq!(got, Ok(()));
    // The root's bytes change between the plan and the run: nothing is trusted.
    std::fs::write(ca.root_path(), "swapped").unwrap();
    let (got, asked) = person_run(&add.as_user, UserPart::Trust, "ok");
    assert!(
        matches!(&got, Err(UserRefused::Failed(UserPart::Trust, why)) if why.contains("changed since it was planned")),
        "{got:?}"
    );
    assert_eq!(asked, "", "security was never asked");
    let _ = std::fs::remove_dir_all(&home);
}

/// The bytes check works with or without `shasum`: on a PATH that lacks it, openssl's digest gives the same hash. A
/// changed file is refused either way.
#[cfg(unix)]
#[test]
fn the_bytes_check_needs_no_shasum() {
    use super::super::trust_scripts::{file_sha256, same_file};
    if !std::path::Path::new("/usr/bin/openssl").is_file() {
        eprintln!("skipped: no /usr/bin/openssl here");
        return;
    }
    let dir = std::env::temp_dir().join(format!("doze-no-shasum-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let cut = ["/usr/bin/cut", "/bin/cut"]
        .into_iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap();
    std::os::unix::fs::symlink(cut, dir.join("bin").join("cut")).unwrap();
    let file = dir.join("root.crt");
    std::fs::write(&file, "the root's bytes\n").unwrap();
    let sha = file_sha256(&file).unwrap();
    let check = |path: &str| {
        Command::new("/bin/sh")
            .env("PATH", path)
            .arg("-c")
            .arg(format!(
                "{}\necho same",
                same_file(&file.display().to_string(), &sha)
            ))
            .output()
            .unwrap()
    };
    let bin = dir.join("bin").display().to_string();
    for path in [bin.as_str(), "/usr/bin:/bin"] {
        let out = check(path);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "same\n",
            "PATH={path}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    std::fs::write(&file, "swapped\n").unwrap();
    for path in [bin.as_str(), "/usr/bin:/bin"] {
        let out = check(path);
        assert!(
            !out.status.success(),
            "PATH={path}: a changed file is refused"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
