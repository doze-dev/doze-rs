use std::time::{Duration, SystemTime};

use doze_display::{self as words, Kind, Outcome, Tone};

use super::{folds, mb_figure};

#[test]
fn megabytes_round() {
    assert_eq!(mb_figure(64_487_424), 62);
    assert_eq!(mb_figure(0), 0);
}

fn task(name: &str, ran: Option<(Outcome, u64)>, now: SystemTime) -> words::Instance {
    words::Instance {
        name: name.into(),
        kind: Kind::Task,
        task: ran.map(|(outcome, secs)| words::TaskRecord {
            outcome,
            exit_code: 0,
            took: Duration::ZERO,
            ran_at: Some(now - Duration::from_secs(secs)),
            why: String::new(),
        }),
        ..Default::default()
    }
}

/// Go `flow.fold`: the tasks drawn as cards (failed or running) leave the fold; ✓ only when the rest all ran,
/// else ▸ in dim; the title says each one as Go's `taskLine` does.
#[test]
fn a_fold_is_go_s_mark_and_title() {
    let now = SystemTime::now();
    let app = |needs: &[&str]| words::Instance {
        name: "api".into(),
        kind: Kind::App,
        needs: needs.iter().map(|s| (*s).to_owned()).collect(),
        ..Default::default()
    };
    let snap = |needs: &[&str]| words::Snapshot {
        instances: vec![
            app(needs),
            task("migrate", Some((Outcome::Done, 16 * 60)), now),
            task("seed", None, now),
            task("broken", Some((Outcome::Failed, 60)), now),
        ],
        ..Default::default()
    };
    let f = folds(&snap(&["migrate", "seed", "broken"]), now);
    let api = f.get("api").expect("a fold");
    assert_eq!((api.glyph, api.tone), ("▸", Tone::Dim));
    assert_eq!(api.title, "before it: migrate ✓ 16m ago · seed not run yet");
    let f = folds(&snap(&["migrate"]), now);
    let api = f.get("api").expect("a fold");
    assert_eq!((api.glyph, api.tone), ("✓", Tone::Good));
    assert!(folds(&snap(&["broken"]), now).is_empty());
}
