//! Transcript tests: each file in tests/transcripts/ is a scenario (a sandbox set up by `@` lines, then commands) and
//! what doze printed for each command, three times over: plain (a pipe), as JSON (`--json`), and as a person at a
//! terminal sees it (`@ tty` files; colour drawn as ⟨…⟩). Plain and JSON are the stable contracts of rule 6.
//! `DOZE_BLESS=1 cargo test -p doze-cli --test transcripts` rewrites the files from what doze prints now.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/setup.rs"]
mod setup;

use doze_cli::Terminals;
use fixture::Sandbox;
use std::fmt::Write as _;
use std::path::Path;

/// One scenario file.
struct Scenario {
    head: Vec<String>,
    setup: Vec<String>,
    commands: Vec<String>,
}

fn parse(text: &str) -> Scenario {
    let mut s = Scenario {
        head: vec![],
        setup: vec![],
        commands: vec![],
    };
    for line in text.lines() {
        if let Some(c) = line.strip_prefix("$ doze") {
            s.commands.push(c.trim().to_string());
        } else if let Some(d) = line.strip_prefix("@ ") {
            s.setup.push(d.to_string());
        } else if line.starts_with('#') && s.commands.is_empty() {
            s.head.push(line.to_string());
        }
    }
    s
}

fn has(s: &Scenario, d: &str) -> bool {
    s.setup.iter().any(|x| x == d)
}

/// Paths, platform and timing that differ between machines, made the same.
fn normalize(text: &str, sb: &Sandbox) -> String {
    let root = sb.root.display().to_string();
    let os_arch = doze_cli::platform::os_arch();
    let (os, arch) = os_arch.split_once('/').unwrap();
    // The sandbox as the system resolves it too (/tmp is /private/tmp on a Mac).
    let canon = sb
        .root
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| root.clone());
    let text = if canon == root {
        text.to_owned()
    } else {
        text.replace(&canon, "{sandbox}")
    };
    let mut t = text
        .replace(&root.replace('\\', "\\\\"), "{sandbox}")
        .replace(&root, "{sandbox}")
        .replace(&doze_cli::platform::triple(), "{triple}")
        .replace(&os_arch, "{os}/{arch}")
        .replace(&format!("\"os\": \"{os}\""), "\"os\": \"{os}\"")
        .replace(&format!("\"arch\": \"{arch}\""), "\"arch\": \"{arch}\"")
        .replace("\x1b[", "⟨")
        .replace('\r', "⟨cr⟩");
    // Windows paths under the sandbox read as Unix ones.
    t = t
        .lines()
        .map(|l| {
            if l.contains("{sandbox}") {
                l.replace("\\\\", "/").replace('\\', "/")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let t: String = t
        .lines()
        .map(
            |l| match l.trim_start().strip_prefix("\"cached_seconds_ago\": ") {
                Some(rest) => l.replace(rest.trim_end_matches(','), "{n}"),
                None => l.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join("\n");
    if text.ends_with('\n') { t + "\n" } else { t }
}

fn section(out: &mut String, title: &str, body: &str) {
    if body.is_empty() {
        return;
    }
    let _ = writeln!(out, "{title}:");
    for l in body.lines() {
        let _ = writeln!(out, "| {l}");
    }
}

/// Runs every command of a scenario the three ways and writes the transcript.
fn transcript(s: &Scenario) -> String {
    let mut out = s.head.join("\n");
    out.push('\n');
    for d in &s.setup {
        let _ = writeln!(out, "@ {d}");
    }
    let tty = has(s, "tty");
    let modes: Vec<(&str, Terminals, bool)> = [
        Some(("", Terminals::default(), false)),
        (!has(s, "no-json")).then_some(("json ", Terminals::default(), true)),
        tty.then_some((
            "tty ",
            Terminals {
                stdin: true,
                stdout: true,
                stderr: true,
            },
            false,
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut results: Vec<Vec<String>> = vec![Vec::new(); s.commands.len()];
    for (label, term, json) in modes {
        let (sb, host, _daemon) = setup::prepare(&s.setup);
        for (i, c) in s.commands.iter().enumerate() {
            let mut args: Vec<&str> = c.split_whitespace().collect();
            if json && !args.contains(&"--json") {
                args.push("--json");
            }
            let ran = fixture::run(&sb, &host, term, has(s, "registry"), &args);
            let mut r = String::new();
            let _ = writeln!(r, "{label}exit {}", ran.code);
            if json {
                section(&mut r, "json", &normalize(&ran.stdout, &sb));
                section(&mut r, "json stderr", &normalize(&ran.stderr, &sb));
            } else {
                section(
                    &mut r,
                    &format!("{label}stdout"),
                    &normalize(&ran.stdout, &sb),
                );
                section(
                    &mut r,
                    &format!("{label}stderr"),
                    &normalize(&ran.stderr, &sb),
                );
                let spawned: Vec<String> = host
                    .spawned
                    .borrow_mut()
                    .drain(..)
                    .map(|a| a.join(" "))
                    .collect();
                section(
                    &mut r,
                    &format!("{label}spawned"),
                    &normalize(&spawned.join("\n"), &sb),
                );
                let opened: Vec<String> = host.opened.borrow_mut().drain(..).collect();
                section(&mut r, &format!("{label}opened"), &opened.join("\n"));
            }
            results[i].push(r);
        }
    }
    for (c, r) in s.commands.iter().zip(results) {
        let _ = writeln!(out, "\n$ {}", format!("doze {c}").trim_end());
        out.push_str(&r.concat());
    }
    out
}

#[test]
fn transcripts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/transcripts");
    let bless = std::env::var_os("DOZE_BLESS").is_some();
    let daemons = setup::sockets();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut failed = Vec::new();
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "txt"))
    {
        let want = std::fs::read_to_string(f).unwrap().replace("\r\n", "\n");
        // A scenario with a running daemon needs the local control socket, which Windows has not yet.
        if !daemons && want.lines().any(|l| l.starts_with("@ daemon")) {
            continue;
        }
        let got = transcript(&parse(&want));
        if got != want {
            if bless {
                std::fs::write(f, &got).unwrap();
            } else {
                failed.push(format!("--- {}\n{}", f.display(), diff(&want, &got)));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "transcripts differ (DOZE_BLESS=1 rewrites them):\n{}",
        failed.join("\n")
    );
    assert!(files.len() >= 20, "the transcripts are missing");
}

/// The first lines that differ, enough to see what moved.
fn diff(want: &str, got: &str) -> String {
    let (w, g): (Vec<_>, Vec<_>) = (want.lines().collect(), got.lines().collect());
    let at = w
        .iter()
        .zip(&g)
        .position(|(a, b)| a != b)
        .unwrap_or(w.len().min(g.len()));
    let from = at.saturating_sub(2);
    format!(
        "want:\n{}\ngot:\n{}",
        w.get(from..(at + 6).min(w.len()))
            .unwrap_or_default()
            .join("\n"),
        g.get(from..(at + 6).min(g.len()))
            .unwrap_or_default()
            .join("\n")
    )
}
