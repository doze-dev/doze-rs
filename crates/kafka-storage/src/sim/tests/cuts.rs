use super::*;

#[test]
fn a_cut_that_keeps_nothing_leaves_what_was_flushed() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let after = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(bytes(&after, "a"), Some(b"safe".to_vec()));
}

#[test]
fn a_cut_that_keeps_everything_and_a_process_kill_lose_nothing() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let kept = fs.power_cut(cut(DataCut::Keep, NameCut::Volatile, 1));
    assert_eq!(bytes(&kept, "a"), Some(b"safe-loose".to_vec()));
    assert_eq!(
        bytes(&fs.process_restart(), "a"),
        Some(b"safe-loose".to_vec())
    );
}

#[test]
fn unflushed_writes_may_read_back_as_zeros() {
    let fs = disk();
    let _file = file_with(&fs, "a", b"safe", b"-loose");
    let after = fs.power_cut(cut(DataCut::Zero, NameCut::Durable, 1));
    assert_eq!(bytes(&after, "a"), Some(b"safe\0\0\0\0\0\0".to_vec()));
}

#[test]
fn a_prefix_cut_keeps_the_flushed_bytes_and_some_of_the_rest() {
    let loose: Vec<u8> = (0..200u8).collect();
    for seed in 0..40 {
        let fs = disk();
        let _file = file_with(&fs, "a", b"safe", &loose);
        let after = bytes(
            &fs.power_cut(cut(DataCut::Prefix, NameCut::Durable, seed)),
            "a",
        )
        .unwrap();
        assert!(after.starts_with(b"safe"), "seed {seed}");
        assert!(after.len() <= 4 + loose.len(), "seed {seed}");
        assert_eq!(
            after.get(4..),
            loose.get(..after.len() - 4),
            "a prefix, byte for byte (seed {seed})"
        );
    }
}

#[test]
fn a_page_cut_keeps_the_flushed_bytes_when_it_only_appends() {
    let loose = vec![7u8; 3 * 4096];
    for seed in 0..40 {
        let fs = disk();
        let _file = file_with(&fs, "a", b"safe", &loose);
        let after = bytes(
            &fs.power_cut(cut(DataCut::Pages, NameCut::Durable, seed)),
            "a",
        )
        .unwrap();
        assert!(after.starts_with(b"safe"), "seed {seed}");
        assert!(
            after.iter().skip(4).all(|b| *b == 7 || *b == 0),
            "pages are whole or absent (seed {seed})"
        );
    }
}

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
fn an_unflushed_removal_and_rename_are_undone() {
    let fs = disk();
    let _a = file_with(&fs, "a", b"A", b"");
    let _b = file_with(&fs, "b", b"B", b"");
    fs.remove_file(&path("a")).unwrap();
    fs.rename(&path("b"), &path("c")).unwrap();
    let after = fs.power_cut(cut(DataCut::Drop, NameCut::Durable, 1));
    assert_eq!(
        bytes(&after, "a"),
        Some(b"A".to_vec()),
        "the removed file is back"
    );
    assert_eq!(bytes(&after, "b"), Some(b"B".to_vec()));
    assert_eq!(bytes(&after, "c"), None);
    let all = fs.power_cut(cut(DataCut::Drop, NameCut::Volatile, 1));
    assert_eq!(
        (bytes(&all, "a"), bytes(&all, "c")),
        (None, Some(b"B".to_vec()))
    );
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
fn a_cut_starts_a_new_process_with_a_clean_slate() {
    let fs = disk();
    fs.halt_after(0);
    let after = fs.power_cut(cut(DataCut::Keep, NameCut::Volatile, 1));
    assert!(
        after.open_or_create(&path("fresh")).is_ok(),
        "faults and the halt do not carry over"
    );
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
