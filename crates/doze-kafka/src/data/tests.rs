use std::path::PathBuf;

use clap::Parser;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-data-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn args(words: &[&str], dir: &std::path::Path) -> DataArgs {
    let mut all = vec!["data".to_owned()];
    all.extend(words.iter().map(|w| (*w).to_owned()));
    all.push("--data-dir".to_owned());
    all.push(dir.display().to_string());
    DataArgs::try_parse_from(all).unwrap()
}

fn said(a: &DataArgs) -> (Result<()>, String) {
    let mut out = Vec::new();
    let done = execute(a, &mut out);
    (done, String::from_utf8(out).unwrap())
}

#[test]
fn the_command_line_asks_for_what_each_action_needs() {
    let dir = scratch("parse");
    for bad in [
        &["empty"][..],
        &["erase"],
        &["empty", "--topic", "t", "--all-topics"],
    ] {
        let mut all = vec!["data"];
        all.extend_from_slice(bad);
        assert!(DataArgs::try_parse_from(&all).is_err(), "{bad:?}");
    }
    assert!(matches!(
        args(&["erase", "--everything"], &dir).action,
        Some(Action::Erase { yes: false, .. })
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_data_directory_that_is_not_there_is_said_and_not_made() {
    let dir = scratch("missing").join("nowhere");
    let (done, _) = said(&args(&[], &dir));
    assert!(
        done.unwrap_err()
            .to_string()
            .contains("there is no data directory")
    );
    assert!(!dir.exists());
}

#[test]
fn with_the_broker_running_it_reads_the_files_alone_and_the_actions_are_refused() {
    let dir = scratch("running");
    std::fs::create_dir_all(dir.join("orders-0")).unwrap();
    std::fs::write(dir.join("orders-0/00000000000000000000.log"), [1_u8; 2048]).unwrap();
    std::fs::write(dir.join("meta.redb"), [0_u8; 100]).unwrap();
    let held = doze_runtime::DataDirLock::take(&dir, "doze-kafka").unwrap();
    let (done, text) = said(&args(&[], &dir));
    done.unwrap();
    assert!(
        text.contains("doze-kafka is running on this data directory"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("orders") && l.contains("2.0 KiB")),
        "{text}"
    );
    for action in [
        &["empty", "--topic", "orders"][..],
        &["erase", "--everything", "--yes"],
    ] {
        let (done, _) = said(&args(action, &dir));
        assert!(
            done.unwrap_err().to_string().contains("is running on"),
            "{action:?}"
        );
    }
    assert!(
        dir.join("orders-0/00000000000000000000.log").exists(),
        "nothing was touched"
    );
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_the_broker_stopped_it_says_everything_and_the_wide_actions_wait_for_yes() {
    let dir = scratch("stopped");
    let (done, text) = said(&args(&[], &dir));
    done.unwrap();
    assert!(text.contains("Metadata store"), "{text}");
    assert!(text.contains("No topics."), "{text}");
    let (done, text) = said(&args(&["erase", "--everything"], &dir));
    assert!(
        done.unwrap_err()
            .to_string()
            .contains("--yes was not given")
    );
    assert!(text.contains("Run it again with --yes"), "{text}");
    let (done, text) = said(&args(&["erase", "--everything", "--yes"], &dir));
    done.unwrap();
    assert!(text.starts_with("Erased 0 topics"), "{text}");
    let (done, _) = said(&args(&["empty", "--topic", "nope"], &dir));
    assert_eq!(
        done.unwrap_err().to_string(),
        "There is no topic called nope."
    );
    let _ = std::fs::remove_dir_all(&dir);
}
