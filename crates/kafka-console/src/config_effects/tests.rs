use super::*;

/// The 33 configs Kafka 4.3.1 lists for a topic, as `topic_config` lists them: each is covered by the table.
const LISTED: [&str; 33] = [
    "compression.type",
    "remote.log.delete.on.disable",
    "leader.replication.throttled.replicas",
    "remote.storage.enable",
    "min.insync.replicas",
    "segment.jitter.ms",
    "remote.log.copy.disable",
    "local.retention.ms",
    "cleanup.policy",
    "flush.ms",
    "follower.replication.throttled.replicas",
    "compression.lz4.level",
    "segment.bytes",
    "retention.ms",
    "compression.gzip.level",
    "flush.messages",
    "compression.zstd.level",
    "max.compaction.lag.ms",
    "file.delete.delay.ms",
    "max.message.bytes",
    "min.compaction.lag.ms",
    "message.timestamp.type",
    "local.retention.bytes",
    "preallocate",
    "index.interval.bytes",
    "min.cleanable.dirty.ratio",
    "unclean.leader.election.enable",
    "retention.bytes",
    "delete.retention.ms",
    "message.timestamp.after.max.ms",
    "message.timestamp.before.max.ms",
    "segment.ms",
    "segment.index.bytes",
];

const PACE: ConfigPace = ConfigPace {
    cleaner_secs: 15,
    cleaner_on: true,
    retention_secs: 300,
};

#[test]
fn every_listed_key_has_a_moment_and_a_sentence_and_a_name_that_is_not_listed_has_none() {
    for key in LISTED {
        let m = moment(key).unwrap_or_else(|| panic!("{key} has no moment"));
        assert!(!sentence(m, Some(PACE)).is_empty(), "{key}");
        assert!(!takes_effect(m, Some(PACE)).is_empty(), "{key}");
        assert!(sentence_of(key, None).is_some(), "{key}");
    }
    for bad in [
        "",
        "retention",
        "Retention.ms",
        "retention.ms ",
        "log.retention.ms",
        "x\nretention.ms",
    ] {
        assert_eq!(moment(bad), None, "{bad:?}");
        assert_eq!(sentence_of(bad, None), None, "{bad:?}");
    }
}

#[test]
fn the_moments_are_what_the_broker_does() {
    for (key, want) in [
        ("cleanup.policy", Moment::NextRequest),
        ("max.message.bytes", Moment::NextRequest),
        ("compression.type", Moment::NextRequest),
        ("segment.bytes", Moment::NextAppend),
        ("index.interval.bytes", Moment::NextAppend),
        ("min.cleanable.dirty.ratio", Moment::CleanerLook),
        ("min.compaction.lag.ms", Moment::CleanerLook),
        ("max.compaction.lag.ms", Moment::CleanerLook),
        ("delete.retention.ms", Moment::CleanerLook),
        ("retention.ms", Moment::RetentionPass),
        ("retention.bytes", Moment::RetentionPass),
        ("preallocate", Moment::Kept(Kept::NotActedOn)),
        ("flush.ms", Moment::Kept(Kept::NotActedOn)),
        ("min.insync.replicas", Moment::Kept(Kept::OneBroker)),
        ("remote.storage.enable", Moment::Kept(Kept::NoTieredStorage)),
    ] {
        assert_eq!(moment(key), Some(want), "{key}");
    }
}

#[test]
fn the_pace_is_the_brokers_own_and_is_said_in_the_boards_words() {
    assert_eq!(pace_word(15), "15 s");
    assert_eq!(pace_word(59), "59 s");
    assert_eq!(pace_word(60), "1 m");
    assert_eq!(pace_word(300), "5 m");
    assert_eq!(pace_word(3600), "1 h");
    assert_eq!(
        sentence(Moment::CleanerLook, Some(PACE)),
        "A change counts at the cleaner's next look, within 15 s."
    );
    assert_eq!(
        sentence(Moment::RetentionPass, Some(PACE)),
        "A change counts at the next retention pass, within 5 m."
    );
    let slow = ConfigPace {
        cleaner_secs: 120,
        ..PACE
    };
    assert!(sentence(Moment::CleanerLook, Some(slow)).ends_with("within 2 m."));
    // A caller that may not read the pace is told no number.
    assert_eq!(
        sentence(Moment::CleanerLook, None),
        "A change counts at the cleaner's next look."
    );
    assert_eq!(
        takes_effect(Moment::RetentionPass, None),
        "at the next retention pass"
    );
}

#[test]
fn a_cleaner_that_is_off_says_that_no_change_counts() {
    let off = ConfigPace {
        cleaner_on: false,
        ..PACE
    };
    assert!(sentence(Moment::CleanerLook, Some(off)).contains("off"));
    assert!(takes_effect(Moment::CleanerLook, Some(off)).starts_with("never"));
    assert_eq!(page_moment(Some(off)), "");
    assert_eq!(page_moment(None), "");
    assert!(page_moment(Some(PACE)).ends_with("within 15 s."));
}

#[test]
fn a_key_the_broker_keeps_and_ignores_says_so_and_invents_no_effect() {
    for key in [
        "preallocate",
        "segment.jitter.ms",
        "min.insync.replicas",
        "local.retention.ms",
    ] {
        let m = moment(key).unwrap();
        let said = sentence(m, Some(PACE));
        assert!(
            said.contains("no use") || said.contains("does nothing"),
            "{key}: {said}"
        );
        assert!(takes_effect(m, Some(PACE)).starts_with("never"), "{key}");
        assert_eq!(when_label(m, Some(PACE)), "never");
    }
}

#[test]
fn the_story_of_a_save_has_a_time_and_a_reader_for_each_moment() {
    assert_eq!(when_label(Moment::CleanerLook, Some(PACE)), "within 15 s");
    assert_eq!(when_label(Moment::RetentionPass, Some(PACE)), "within 5 m");
    assert_eq!(when_label(Moment::NextRequest, None), "next request");
    assert_eq!(when_label(Moment::NextAppend, None), "next append");
    assert_eq!(when_label(Moment::CleanerLook, None), "next look");
    assert!(reader(Moment::CleanerLook).contains("cleaner"));
    assert!(sees(Moment::CleanerLook, Some(PACE)).ends_with("within 15 s"));
}

#[test]
fn the_keys_the_boards_teach_have_their_words() {
    for key in [
        "min.cleanable.dirty.ratio",
        "min.compaction.lag.ms",
        "cleanup.policy",
    ] {
        assert!(what_it_is(key).is_some(), "{key}");
    }
    assert_eq!(what_it_is("preallocate"), None);
}
