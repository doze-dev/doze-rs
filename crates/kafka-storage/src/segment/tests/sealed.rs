use super::*;

#[test]
fn a_sealed_segment_keeps_what_the_active_one_knew() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 20).stamped(offset(0), None), 1_000)
        .unwrap();
    seg.append(&batch(1, -1).stamped(offset(1), None), 2_000)
        .unwrap(); // -1: no timestamp
    seg.append(&batch(1, 30).stamped(offset(2), None), 3_000)
        .unwrap();
    seg.publish();
    seg.sync().unwrap();
    let sealed = seg.into_sealed();
    assert_eq!(
        (sealed.base(), sealed.size(), sealed.modified_ms()),
        (offset(0), 3 * one(), 3_000)
    );
    assert_eq!(sealed.max_timestamp().unwrap().map(|t| t.get()), Some(30));
    assert_eq!(
        sealed.view().read(0, 10_000).unwrap().len() as u64,
        3 * one()
    );
}

#[test]
fn a_sealed_segment_read_back_finds_its_newest_timestamp_when_asked() {
    let (fs, dir) = disk();
    let mut seg = active(&fs, &dir, 0, INTERVAL);
    seg.append(&batch(1, 20).stamped(offset(0), None), 1)
        .unwrap();
    seg.append(&batch(1, 40).stamped(offset(1), None), 2)
        .unwrap();
    seg.publish();
    seg.sync().unwrap();
    drop(seg);
    let sealed = SealedSegment::open(&fs, &dir, offset(0), INTERVAL).unwrap();
    assert_eq!(sealed.size(), 2 * one());
    assert_eq!(sealed.max_timestamp().unwrap().map(|t| t.get()), Some(40));
    assert!(SealedSegment::open(&fs, &dir, offset(99), INTERVAL).is_err());
}
