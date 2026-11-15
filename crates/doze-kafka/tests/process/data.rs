//! `doze-kafka data` against the real binary (docs/port/data-control.md): while the broker runs it reads the files alone and
//! refuses to act; stopped, it empties a topic so that the broker comes back with the topic and its next offset, and erases
//! everything once `--yes` is given.

use super::*;

fn data(dir: &Path, words: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_doze-kafka"))
        .arg("data")
        .args(words)
        .arg("--data-dir")
        .arg(dir)
        .output_within(PROCESS_WAIT)
}

fn text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn data_reads_a_running_broker_and_empties_and_erases_a_stopped_one() {
    let dir = scratch("data");
    let mut broker = start(&dir);
    let mut s = broker.connect();
    create(&mut s, "orders");
    create(&mut s, "audit");
    for _ in 0..4 {
        produce(&mut s, "orders", batch());
    }
    produce(&mut s, "audit", batch());

    let running = data(&dir, &[]);
    assert!(running.status.success(), "{}", text(&running));
    assert!(text(&running).contains("doze-kafka is running on this data directory"));
    assert!(
        text(&running).lines().any(|l| l.starts_with("orders")),
        "{}",
        text(&running)
    );
    let refused = data(&dir, &["empty", "--topic", "orders"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused).contains("is running on"),
        "{}",
        text(&refused)
    );
    drop(s);
    assert!(broker.terminate().success());

    let report = data(&dir, &[]);
    assert!(report.status.success(), "{}", text(&report));
    let said = text(&report);
    assert!(said.contains("UNBOUNDED"), "{said}");
    assert!(
        said.contains("No retention limit: it keeps everything"),
        "{said}"
    );
    let emptied = data(&dir, &["empty", "--topic", "orders"]);
    assert!(emptied.status.success(), "{}", text(&emptied));
    assert!(
        text(&emptied).starts_with("Emptied orders: 12 records in 1 partition,"),
        "{}",
        text(&emptied)
    );

    // The broker comes back with the topic, empty, and the next record takes the next offset.
    let again = start(&dir);
    let mut s = again.connect();
    assert_eq!(produce(&mut s, "orders", batch()), 12);
    drop(s);
    drop(again);

    let unsure = data(&dir, &["erase", "--everything"]);
    assert_eq!(unsure.status.code(), Some(1));
    assert!(
        text(&unsure).contains("This would delete 2 topics"),
        "{}",
        text(&unsure)
    );
    let erased = data(&dir, &["erase", "--everything", "--yes"]);
    assert!(erased.status.success(), "{}", text(&erased));
    assert!(
        text(&erased).starts_with("Erased 2 topics"),
        "{}",
        text(&erased)
    );
    let after = data(&dir, &[]);
    assert!(text(&after).contains("No topics."), "{}", text(&after));
    let _ = std::fs::remove_dir_all(&dir);
}
