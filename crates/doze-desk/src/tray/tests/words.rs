//! The tray menu's words for an address and for a stack up with everything asleep.

use super::*;

#[test]
fn open_url_takes_the_registrys_address_when_there_is_one() {
    let st = stack(
        "k-web",
        "web",
        true,
        json!([{"name": "front", "kind": "app", "state": "active", "domain": "front.web.doze", "bind": "127.0.0.9:3000"}]),
    );
    let stacks = vec![st];
    let m = crate::tray::build(&stacks, now());
    let plain = menu(&m, &Attention::default(), &stacks, &|_| None);
    assert_eq!(
        titles(sub(&plain, "●  web"))[1],
        "Open http://front.web.doze:3000"
    );
    let fronted = |h: &str| (h == "front.web.doze").then(|| "http://front.web.doze".to_owned());
    let e = menu(&m, &Attention::default(), &stacks, &fronted);
    assert_eq!(titles(sub(&e, "●  web"))[1], "Open http://front.web.doze");
}

#[test]
fn a_stack_up_with_everything_asleep_says_it_wakes_on_use() {
    let asleep = json!([
        {"name": "api", "kind": "app", "state": "stopped"},
        {"name": "db", "kind": "service", "state": "stopped"},
        {"name": "seed", "kind": "task", "state": "stopped"}
    ]);
    let st = stack("k", "shop", true, asleep);
    assert!(st.all_asleep());
    assert_eq!(
        crate::tray::build(&[st], now()).lines[0].sub,
        crate::ALL_ASLEEP
    );
    let one_up = json!([
        {"name": "api", "kind": "app", "state": "active"},
        {"name": "db", "kind": "service", "state": "stopped"}
    ]);
    let st = stack("k", "shop", true, one_up);
    assert!(!st.all_asleep());
    assert_eq!(crate::tray::build(&[st], now()).lines[0].sub, "1 awake");
    // Stopped is not asleep, and neither is a stack whose daemon is not answering (what it shows is old).
    let mut st = stack(
        "k",
        "shop",
        false,
        json!([{"name": "api", "kind": "app", "state": "stopped"}]),
    );
    assert!(!st.all_asleep());
    st.running = true;
    st.link.conn = Conn::Connecting;
    assert!(!st.all_asleep());
}
