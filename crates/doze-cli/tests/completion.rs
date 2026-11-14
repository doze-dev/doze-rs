//! Dynamic completions (proposal B5) through the real protocol: `COMPLETE=<shell> doze` prints the registration script
//! for each of the four shells, and `COMPLETE=<shell> doze -- doze <words>` answers with the stack's names, read from
//! doze.hcl offline, each with what it is.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/fixture.rs"]
mod fixture;

use doze_cli::Terminals;

fn complete(sb: &fixture::Sandbox, shell: &str, words: &[&str]) -> fixture::Ran {
    let mut host = fixture::FakeHost::default();
    host.env.insert("COMPLETE".into(), shell.into());
    // Registration is `COMPLETE=<shell> doze`; a request is `COMPLETE=<shell> doze -- doze <words>`.
    let mut args = if words.is_empty() {
        vec![]
    } else {
        vec!["--", "doze"]
    };
    args.extend_from_slice(words);
    fixture::run(sb, &host, Terminals::default(), false, &args)
}

#[test]
fn names_complete_from_doze_hcl_in_every_shell() {
    let sb = fixture::Sandbox::new("complete");
    let project = sb.project();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("doze.hcl"),
        "module valkey \"cache\" { version = 9 }\napp \"api\" {\n  run = \"x\"\n}\ntask \"seed\" {\n  run = \"s\"\n  manual = true\n}\n",
    )
    .unwrap();
    // The value completers read the stack where the shell is.
    std::env::set_current_dir(&project).unwrap();

    for shell in ["bash", "zsh", "fish", "powershell"] {
        let reg = complete(&sb, shell, &[]);
        assert_eq!(reg.code, 0, "{shell}: {}", reg.stderr);
        assert!(reg.stdout.contains("COMPLETE"), "{shell}: {}", reg.stdout);
        // `doze completion <shell>` is the same script.
        let mut host = fixture::FakeHost::default();
        host.env.remove("COMPLETE");
        let via = fixture::run(
            &sb,
            &host,
            Terminals::default(),
            false,
            &["completion", shell],
        );
        assert_eq!(via.stdout, reg.stdout, "{shell}");
    }

    // fish and powershell take the words as they are.
    let fish = complete(&sb, "fish", &["sleep", ""]);
    assert!(fish.stdout.contains("cache\tvalkey"), "{}", fish.stdout);
    assert!(fish.stdout.contains("api\tapp"), "{}", fish.stdout);
    let tasks = complete(&sb, "fish", &["task", ""]);
    assert!(
        tasks.stdout.contains("seed\ttask") && !tasks.stdout.contains("cache"),
        "{}",
        tasks.stdout
    );
    let ps = complete(&sb, "powershell", &["logs", "a"]);
    assert!(ps.stdout.contains("api"), "{}", ps.stdout);

    // A shell doze does not know is a usage error, never a stack trace.
    let other = complete(&sb, "tcsh", &[]);
    assert_eq!(other.code, 2);
}

/// The engine offers every command by its own name (an alias only when it alone matches), never the hidden ones, and
/// each command's flags.
#[test]
fn commands_complete_by_their_own_names() {
    let sb = fixture::Sandbox::new("complete-commands");
    let all = complete(&sb, "fish", &[""]).stdout;
    for word in ["status", "modules", "dns-setup", "connect", "wake", "pin"] {
        assert!(
            all.lines().any(|l| l.starts_with(&format!("{word}\t"))),
            "no {word}:\n{all}"
        );
    }
    for hidden in ["__refresh", "ls\t", "mod\t"] {
        assert!(!all.contains(hidden), "offers {hidden}:\n{all}");
    }
    assert!(
        complete(&sb, "fish", &["kee"])
            .stdout
            .starts_with("keepawake")
    );
    assert!(
        complete(&sb, "fish", &["wake", "--d"])
            .stdout
            .contains("--dry-run")
    );
}
