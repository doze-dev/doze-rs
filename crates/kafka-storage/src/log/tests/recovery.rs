use super::*;
use crate::fs::{Fs, FsFile};
use crate::sim::{Cut, DataCut, NameCut};
use crate::testutil::{offset, raw};
use kafka_wire::batch::TimestampType;

#[test]
fn reopening_restores_the_log_byte_for_byte_and_appends_continue() {
    let (fs, dir) = written(5, NEVER);
    let before = fs.contents(&dir.join(LOG0)).unwrap();
    let mut log = reopen(&fs, &dir);
    assert_eq!(
        (log.start_offset(), log.end_offset()),
        (offset(0), offset(5))
    );
    assert_eq!(all(&log.reader()), before);
    assert_eq!(
        log.append(&ones(1, 0), NOW, TimestampType::Create)
            .unwrap()
            .first_offset,
        offset(5)
    );
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn a_torn_tail_is_cut_and_the_cut_is_flushed() {
    let (fs, dir) = written(3, NEVER);
    let whole = fs.contents(&dir.join(LOG0)).unwrap().len() as u64;
    let file = fs.open_existing(&dir.join(LOG0)).unwrap();
    file.write_all_at(&raw(3, 1, 0)[..40], whole).unwrap(); // an append that never finished
    let log = reopen(&fs, &dir);
    assert_eq!(log.end_offset(), offset(3));
    assert_eq!(
        fs.contents(&dir.join(LOG0)).unwrap().len() as u64,
        whole,
        "the torn bytes are gone"
    );
    // A second crash right after does not bring them back.
    let again = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 1,
    });
    assert_eq!(again.contents(&dir.join(LOG0)).unwrap().len() as u64, whole);
}

#[test]
fn a_zero_filled_tail_after_a_power_cut_is_cut() {
    let (fs, dir, mut log) = fresh(NEVER);
    log.append(&ones(2, 0), NOW, TimestampType::Create).unwrap();
    log.sync().unwrap();
    log.append(&ones(3, 0), NOW, TimestampType::Create).unwrap();
    let after = fs.power_cut(Cut {
        data: DataCut::Zero,
        names: NameCut::Durable,
        seed: 1,
    });
    assert!(
        after.contents(&dir.join(LOG0)).unwrap().len() > (2 * one()) as usize,
        "the length survived, the data did not"
    );
    let log = open(&after, &dir, NEVER, Trust::Unvouched);
    assert_eq!(log.end_offset(), offset(2));
}

#[test]
fn a_damaged_batch_in_the_middle_ends_the_log_there() {
    let (fs, dir) = written(4, NEVER);
    let file = fs.open_existing(&dir.join(LOG0)).unwrap();
    let middle = 2 * one() + 40;
    file.write_all_at(&[0xff; 4], middle).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(
        log.end_offset(),
        offset(2),
        "every batch is checked when nothing vouches for the files"
    );
}

#[test]
fn an_index_that_disagrees_with_the_log_is_rebuilt() {
    let (fs, dir) = written(6, NEVER);
    let want = fs.contents(&dir.join(INDEX0)).unwrap();
    let index = fs.open_existing(&dir.join(INDEX0)).unwrap();
    index.write_all_at(&[7; 16], want.len() as u64).unwrap(); // an entry for bytes that are not there
    index.write_all_at(&[1], want.len() as u64 + 16).unwrap(); // and half of another
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(INDEX0)).unwrap(), want);
    assert_eq!(
        bases(&log.reader().read(offset(4), usize::MAX, None).unwrap()),
        vec![4, 5]
    );
    // A missing index file is rebuilt the same way.
    let (fs, dir) = written(6, NEVER);
    fs.remove_file(&dir.join(INDEX0)).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(INDEX0)).unwrap(), want);
    assert_eq!(
        bases(&log.reader().read(offset(5), usize::MAX, None).unwrap()),
        vec![5]
    );
}

#[test]
fn the_index_of_a_sealed_segment_is_rebuilt_when_it_is_gone() {
    let (fs, dir) = written(6, 2 * one());
    let sealed_index = "00000000000000000000.index";
    let want = fs.contents(&dir.join(sealed_index)).unwrap();
    fs.remove_file(&dir.join(sealed_index)).unwrap();
    let log = reopen(&fs, &dir);
    assert_eq!(fs.contents(&dir.join(sealed_index)).unwrap(), want);
    assert_eq!(bases(&all(&log.reader())), vec![0, 1, 2, 3, 4, 5]);
}
