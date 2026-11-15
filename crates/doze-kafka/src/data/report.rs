//! What `doze-kafka data` prints: the data directory by kind, then each topic largest first, then why each holds what it
//! holds. With the broker running, only what the files say.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use kafka_broker::{DataUsage, bytes_words};
use kafka_storage::fs::{Fs, RealFs};
use kafka_storage::usage::walk_data_dir;

/// Columns, each right-aligned (`true`) or left-aligned, padded to its widest cell.
fn table(out: &mut dyn Write, head: &[(&str, bool)], rows: &[Vec<String>]) -> Result<()> {
    let mut widths: Vec<usize> = head.iter().map(|(h, _)| h.len()).collect();
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let parts: Vec<String> = cells
            .iter()
            .zip(&widths)
            .zip(head)
            .map(|((c, w), (_, right))| {
                if *right {
                    format!("{c:>w$}")
                } else {
                    format!("{c:<w$}")
                }
            })
            .collect();
        parts.join("  ").trim_end().to_owned()
    };
    writeln!(out, "{}", line(head.iter().map(|(h, _)| *h).collect()))?;
    for row in rows {
        writeln!(out, "{}", line(row.iter().map(String::as_str).collect()))?;
    }
    Ok(())
}

fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "-".to_owned();
    }
    let tenths = u128::from(part) * 1000 / u128::from(whole);
    format!("{}.{}%", tenths / 10, tenths % 10)
}

fn volume(dir: &Path) -> String {
    match RealFs.space(dir) {
        Ok(Some(s)) => format!(
            "the volume has {} free of {}",
            bytes_words(s.free),
            bytes_words(s.total)
        ),
        _ => "the volume's room is not known here".to_owned(),
    }
}

/// With the broker stopped: everything the broker knows.
pub(super) fn stopped(u: &DataUsage, subjects: Option<usize>, out: &mut dyn Write) -> Result<()> {
    let total = u.total();
    writeln!(out, "Data directory  {}", u.data_dir.display())?;
    writeln!(
        out,
        "On disk         {} ({})",
        bytes_words(total),
        volume(&u.data_dir)
    )?;
    writeln!(out)?;
    let partitions: i64 = u.topics.iter().map(|t| i64::from(t.partitions)).sum();
    let mut held = format!(
        "committed offsets of {} groups, {} transactional ids, ACLs, users",
        u.groups, u.transactional_ids
    );
    if let Some(n) = subjects {
        held.push_str(&format!(", {n} schema subjects"));
    }
    let mut kinds = vec![
        vec![
            "Topics".to_owned(),
            bytes_words(u.topics_bytes()),
            format!("{} topics, {partitions} partitions", u.topics.len()),
        ],
        vec!["Metadata store".to_owned(), bytes_words(u.meta_bytes), held],
    ];
    if !u.strays.is_empty() {
        let names: Vec<&str> = u.strays.iter().map(|s| s.name.as_str()).collect();
        kinds.push(vec![
            "Belonging to no topic".to_owned(),
            bytes_words(u.stray_bytes()),
            names.join(", "),
        ]);
    }
    kinds.push(vec![
        "Other files".to_owned(),
        bytes_words(u.other_bytes),
        format!("{} files", u.other_files),
    ]);
    table(
        out,
        &[("KIND", false), ("SIZE", true), ("WHAT", false)],
        &kinds,
    )?;
    if u.topics.is_empty() {
        writeln!(out, "\nNo topics.")?;
        return Ok(());
    }
    writeln!(out)?;
    let rows: Vec<Vec<String>> = u
        .topics
        .iter()
        .map(|t| {
            vec![
                t.name.clone(),
                bytes_words(t.bytes),
                share(t.bytes, total),
                t.partitions.to_string(),
                t.segments.to_string(),
                if t.keeps.unbounded() {
                    "UNBOUNDED".to_owned()
                } else {
                    String::new()
                },
            ]
        })
        .collect();
    let head = [
        ("TOPIC", false),
        ("SIZE", true),
        ("SHARE", true),
        ("PARTITIONS", true),
        ("SEGMENTS", true),
        ("", false),
    ];
    table(out, &head, &rows)?;
    writeln!(out, "\nWhy each holds what it holds:")?;
    let now = now_ms();
    let width = u
        .topics
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or(0);
    for t in &u.topics {
        let why = t.keeps.why(t.next_drop_ms.map(|d| d - now));
        writeln!(out, "  {:<width$}  {why}", t.name)?;
    }
    writeln!(
        out,
        "\nJudged with the broker's default flags: a broker started with --retention-ms or --retention-bytes bounds the topics that set no retention of their own.\nTo give space back (broker stopped): doze-kafka data empty --topic NAME, data empty --all-topics, data erase --everything."
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// With the broker running: the files alone, nothing opened for writing, the metadata store not opened at all.
pub(super) fn running(dir: &Path, out: &mut dyn Write) -> Result<()> {
    let walked = walk_data_dir(&RealFs, dir)?;
    let mut topics: BTreeMap<&str, (u64, i32, usize)> = BTreeMap::new();
    for p in &walked.partitions {
        let t = topics.entry(p.topic.as_str()).or_default();
        t.0 += p.bytes;
        t.1 += 1;
        t.2 += p.segments;
    }
    let total = walked.total();
    writeln!(
        out,
        "doze-kafka is running on this data directory, so this is what its files say. What each topic keeps and why, and emptying or erasing, are on the console's Data page (/data).\n"
    )?;
    writeln!(out, "Data directory  {}", dir.display())?;
    writeln!(
        out,
        "On disk         {} ({})",
        bytes_words(total),
        volume(dir)
    )?;
    writeln!(out)?;
    let partition_bytes: u64 = walked.partitions.iter().map(|p| p.bytes).sum();
    let kinds = vec![
        vec![
            "Partitions".to_owned(),
            bytes_words(partition_bytes),
            format!(
                "{} directories of {} topics",
                walked.partitions.len(),
                topics.len()
            ),
        ],
        vec![
            "Metadata store".to_owned(),
            bytes_words(walked.meta_bytes),
            "meta.redb".to_owned(),
        ],
        vec![
            "Other files".to_owned(),
            bytes_words(walked.other_bytes),
            format!("{} files", walked.other_files),
        ],
    ];
    table(
        out,
        &[("KIND", false), ("SIZE", true), ("WHAT", false)],
        &kinds,
    )?;
    if topics.is_empty() {
        return Ok(());
    }
    let mut by_size: Vec<(&str, (u64, i32, usize))> = topics.into_iter().collect();
    by_size.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
    let rows: Vec<Vec<String>> = by_size
        .iter()
        .map(|(name, (bytes, parts, segs))| {
            vec![
                (*name).to_owned(),
                bytes_words(*bytes),
                share(*bytes, total),
                parts.to_string(),
                segs.to_string(),
            ]
        })
        .collect();
    writeln!(out)?;
    table(
        out,
        &[
            ("TOPIC", false),
            ("SIZE", true),
            ("SHARE", true),
            ("PARTITIONS", true),
            ("SEGMENTS", true),
        ],
        &rows,
    )
}
