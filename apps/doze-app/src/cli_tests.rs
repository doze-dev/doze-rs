//! The flags each command takes, read as a person types them.

use super::{Cmd, Screen, parse};
use crate::theme::Mode;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn a_shot_reads_every_flag() {
    let Ok(Cmd::Shot(s)) = parse(args(
        "shot --fixture larder --screen help --mode light --size 900x620 --out /tmp/x.png --select api --zoom 0.25 --at 10,20",
    )) else {
        panic!("a shot")
    };
    assert_eq!(s.from.fixture.as_deref(), Some("larder"));
    assert_eq!(
        (s.screen, s.mode, s.size),
        (Screen::Help, Mode::Light, (900., 620.))
    );
    assert_eq!(
        (s.select.as_deref(), s.zoom, s.at),
        (Some("api"), Some(0.25), Some((10., 20.)))
    );
    assert!(parse(args("shot --screen nope")).is_err());
}

#[test]
fn a_shot_inside_a_stack_and_the_logs_bench() {
    let Ok(Cmd::Shot(s)) = parse(args(
        "shot --fixture shop --screen instance --select api --tab logs --hold 8000 --filter x --find-mode --level 2",
    )) else {
        panic!("a shot")
    };
    assert_eq!(s.screen, Screen::Instance);
    assert_eq!(
        (s.tab.as_deref(), s.hold, s.filter.as_deref(), s.level),
        (Some("logs"), Some(8000), Some("x"), Some(2))
    );
    assert!(s.find_mode);
    let Ok(Cmd::Bench(b)) = parse(args("bench --fixture shop --logs --follow 5")) else {
        panic!("a bench")
    };
    assert!(b.logs && b.follow == 5);
}

#[test]
fn the_app_forwards_folders_links_and_show() {
    let Ok(Cmd::App(a)) = parse(args("/code/shop --show db --synth 2000")) else {
        panic!("the app")
    };
    assert_eq!(a.forward, args("/code/shop --show db"));
    assert_eq!(a.from.synth, Some(2000));
    assert!(parse(args("--nope")).is_err());
}
