use super::*;

fn sh(what: &str, script: &str) -> Plan {
    Plan {
        what: what.into(),
        shell: Shell::Sh,
        script: script.into(),
    }
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
