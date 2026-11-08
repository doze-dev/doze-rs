use super::*;

#[test]
fn names_follow_the_directory_flush() {
    let fs = disk();
    let (made, _) = fs.open_or_create(&path("new")).unwrap();
    made.write_all_at(b"data", 0).unwrap();
    made.sync_data().unwrap(); // the contents are flushed; the name is not
    let gone = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(
        bytes(&gone, "new"),
        None,
        "a flushed file vanishes with its unflushed name"
    );
    fs.sync_dir(Path::new(DIR)).unwrap();
    let kept = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(bytes(&kept, "new"), Some(b"data".to_vec()));
}

#[test]
fn random_name_cuts_are_replayable_and_apply_changes_in_order() {
    let make = || {
        let fs = disk();
        for i in 0..6 {
            let _ = file_with(&fs, &format!("f{i}"), b"x", b"");
            fs.rename(&path(&format!("f{i}")), &path(&format!("g{i}")))
                .unwrap();
        }
        fs
    };
    let names = |seed| {
        make()
            .power_cut(cut(DataCut::Drop, NameCut::Random, seed))
            .names(Path::new(DIR))
    };
    assert_eq!(names(5), names(5));
    assert!(
        (0..20)
            .map(names)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    );
    // A name is either its old or its new, never both and never neither.
    for seed in 0..20 {
        let seen = names(seed);
        for i in 0..6 {
            let (old, new) = (format!("f{i}"), format!("g{i}"));
            assert!(
                seen.contains(&old) ^ seen.contains(&new),
                "seed {seed}: {seen:?}"
            );
        }
    }
}

#[test]
fn a_restarted_process_finds_the_page_cache_and_a_later_power_cut_still_takes_what_was_never_flushed()
 {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let restarted = fs.process_restart();
    assert_eq!(
        bytes(&restarted, "a"),
        Some(b"safe-loose".to_vec()),
        "the cache survives the kill"
    );
    // But it is still not on the disk: a power cut after the restart takes it.
    let after = restarted.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(bytes(&after, "a"), Some(b"safe".to_vec()));
    // Names the process had not flushed are likewise still only in memory.
    let (made, _) = restarted.open_or_create(&path("new")).unwrap();
    made.write_all_at(b"x", 0).unwrap();
    made.sync_data().unwrap();
    let again = restarted.process_restart();
    assert_eq!(bytes(&again, "new"), Some(b"x".to_vec()));
    assert_eq!(
        bytes(
            &again.power_cut(cut(DataCut::Drop, NameCut::Durable, 1)),
            "new"
        ),
        None
    );
}
