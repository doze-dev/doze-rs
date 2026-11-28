use std::path::Path;
use std::sync::mpsc;

use doze_fs::fs::Fs;
use doze_fs::sim::{Failure, Op as FsOp, SimFs};

use super::*;
use crate::{GroupCommit, Op, Store, StoreError};

const DATA: &str = "/data";

/// Queued writes for a test: the changes as given, to table `t`.
struct Writes(Vec<(Vec<u8>, Option<Vec<u8>>)>, Vec<(Vec<u8>, Vec<u8>)>);

impl Overlaid for Writes {
    fn changes(&self, table: &str, f: &mut dyn FnMut(Change<'_>)) {
        if table != "t" {
            return;
        }
        for (lo, hi) in &self.1 {
            f(Change::DeleteRange(lo, hi));
        }
        for (k, v) in &self.0 {
            f(match v {
                Some(v) => Change::Put(k, v),
                None => Change::Delete(k),
            });
        }
    }
}

fn put(k: &str, v: &str) -> Arc<dyn Overlaid> {
    Arc::new(Writes(
        vec![(k.as_bytes().to_vec(), Some(v.as_bytes().to_vec()))],
        Vec::new(),
    ))
}

fn writer() -> (SimFs, GroupCommit) {
    let fs = SimFs::new();
    fs.create_dir(Path::new(DATA)).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let store = Store::open(&fs, Path::new(DATA), "t.redb", 1 << 20, |_| Ok(())).unwrap();
    let writer = GroupCommit::start(store).unwrap();
    (fs, writer)
}

/// An operation that holds the writer until `release` is sent: what is submitted after it waits in the queue.
fn gate(writer: &GroupCommit) -> (mpsc::Sender<()>, crate::Ack) {
    let (release, hold) = mpsc::channel::<()>();
    let (started, running) = mpsc::channel::<()>();
    let op: Op = Box::new(move |_| {
        started.send(()).unwrap();
        hold.recv().unwrap();
        Ok(())
    });
    let ack = writer.submit(op);
    running.recv().unwrap();
    (release, ack)
}

fn nothing() -> Op {
    Box::new(|_| Ok(()))
}

#[test]
fn a_read_sees_a_queued_write_until_its_commit_and_then_not_from_the_overlay() {
    let (_fs, writer) = writer();
    let (release, held) = gate(&writer);
    let first = writer.submit_seen(nothing(), put("k", "one"));
    let second = writer.submit_seen(nothing(), put("k", "two"));
    let view = writer.pending().view();
    assert_eq!(
        view.get("t", b"k"),
        Some(Some(b"two".to_vec())),
        "the later write wins"
    );
    assert_eq!(view.get("t", b"other"), None);
    assert_eq!(view.get("u", b"k"), None, "another table is not touched");
    release.send(()).unwrap();
    held.wait().unwrap();
    first.wait().unwrap();
    second.wait().unwrap();
    assert!(
        writer.pending().is_empty(),
        "committed: the file answers now"
    );
}

#[test]
fn a_failed_commit_drops_the_overlay_and_a_later_write_is_not_laid_over() {
    let (fs, writer) = writer();
    let (release, held) = gate(&writer);
    fs.fail(FsOp::Fsync, 1, Failure::Io);
    let doomed = writer.submit_seen(nothing(), put("k", "lost"));
    assert!(writer.pending().view().get("t", b"k").is_some());
    release.send(()).unwrap();
    assert!(held.wait().is_err());
    assert!(matches!(doomed.wait(), Err(StoreError::Failed(_))));
    assert_eq!(
        writer.pending().view().get("t", b"k"),
        None,
        "gone with its commit"
    );
    fs.clear_faults();
    let refused = writer.submit_seen(nothing(), put("k", "refused"));
    assert!(
        writer.pending().is_empty(),
        "the writer refuses the database's writes, so the overlay takes none"
    );
    assert!(refused.wait().is_err());
}

#[test]
fn an_operation_that_fails_alone_leaves_the_overlay_and_its_neighbours_commit() {
    let (_fs, writer) = writer();
    let (release, held) = gate(&writer);
    let bad = writer.submit_seen(
        Box::new(|_| Err(StoreError::Corrupt("refused".into()))),
        put("a", "bad"),
    );
    let good = writer.submit_seen(nothing(), put("b", "good"));
    release.send(()).unwrap();
    held.wait().unwrap();
    assert!(bad.wait().is_err());
    good.wait().unwrap();
    assert!(writer.pending().is_empty());
    // The database still takes writes.
    let (release, held) = gate(&writer);
    drop(writer.submit_seen(nothing(), put("c", "later")));
    assert!(writer.pending().view().get("t", b"c").is_some());
    release.send(()).unwrap();
    held.wait().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn settled_waits_for_the_writes_queued_before_it() {
    let (_fs, writer) = writer();
    let (release, held) = gate(&writer);
    let queued = writer.submit_seen(nothing(), put("k", "v"));
    let pending = writer.pending().clone();
    let waiter = tokio::spawn(async move { pending.settled().await });
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished(), "the write is still queued");
    release.send(()).unwrap();
    tokio::task::spawn_blocking(move || held.wait())
        .await
        .unwrap()
        .unwrap();
    queued.await.unwrap();
    waiter.await.unwrap();
}

/// The file's rows and the queued writes, merged in key order and in reverse; a put after a range delete shows, the rows the range
/// covers do not.
#[test]
fn a_scan_merges_the_file_with_the_queued_writes_in_either_order() {
    let pending = Pending::default();
    let b = |s: &str| s.as_bytes().to_vec();
    pending.add(Arc::new(Writes(
        vec![(b("b2"), Some(b("new"))), (b("a1"), None)],
        Vec::new(),
    )));
    pending.add(Arc::new(Writes(
        vec![(b("c5"), Some(b("back")))],
        vec![(b("c"), b("d"))],
    )));
    pending.add(Arc::new(Writes(vec![(b("e1"), Some(b("e")))], Vec::new())));
    let file: Vec<(Vec<u8>, Vec<u8>)> = ["a0", "a1", "b2", "c1", "c9", "d1"]
        .iter()
        .map(|k| (b(k), b("filed")))
        .collect();
    let view = pending.view();
    let scan = |reverse: bool, limit: usize| {
        let mut rows: Vec<&[u8]> = file.iter().map(|(k, _)| k.as_slice()).collect();
        if reverse {
            rows.reverse();
        }
        let mut out = Vec::new();
        view.merge(
            Span {
                table: "t",
                lo: b"a",
                hi: b"z",
                reverse,
            },
            rows.into_iter().map(Ok::<_, ()>),
            |k| {
                let v = file
                    .iter()
                    .find(|(f, _)| f.as_slice() == *k)
                    .map_or(&[][..], |(_, v)| v);
                (*k, v)
            },
            |k, v| {
                out.push(format!(
                    "{}={}",
                    String::from_utf8_lossy(k),
                    String::from_utf8_lossy(v)
                ));
                out.len() < limit
            },
        )
        .unwrap();
        out
    };
    let forward = scan(false, 100);
    assert_eq!(
        forward,
        ["a0=filed", "b2=new", "c5=back", "d1=filed", "e1=e"]
    );
    let mut backward = scan(true, 100);
    backward.reverse();
    assert_eq!(backward, forward);
    assert_eq!(scan(true, 2), ["e1=e", "d1=filed"]);
    assert_eq!(view.get("t", b"c1"), Some(None), "inside the deleted range");
    assert_eq!(view.get("t", b"c5"), Some(Some(b("back"))));
}
