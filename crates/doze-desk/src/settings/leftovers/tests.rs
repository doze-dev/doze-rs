//! Left behind, against made-up machines: a temporary home, `ps` text, a resolver folder of its own. Nothing here
//! reads or touches the real machine's processes, routes or keychain.

use std::sync::Arc;

use super::*;
use crate::settings::record::Record;
use crate::view::StackView;

const PS: &str = "  101     1 Fri Nov  6 20:00:00 2026 /usr/local/bin/doze __daemon --config /gone/doze.hcl\n\
  102     1 Fri Nov  6 20:01:00 2026 /x/.doze/modules/doze/valkey/1.0.0/bin/valkey-plugin --serve\n\
  103   101 Fri Nov  6 20:02:00 2026 /x/.doze/modules/doze/valkey/1.0.0/bin/valkey-plugin --serve\n\
  104     1 Fri Nov  6 20:03:00 2026 /bin/zsh -l\n\
  garbage line\n";

struct World {
    _home: tempfile::TempDir,
    home: std::path::PathBuf,
    resolver: tempfile::TempDir,
}

fn world() -> World {
    let home = tempfile::tempdir().unwrap();
    let h = home.path().to_path_buf();
    // A listed stack, an orphan folder with some data, and the authority.
    std::fs::create_dir_all(h.join("projects/shop")).unwrap();
    std::fs::write(h.join("projects/shop/project.json"), "{}").unwrap();
    std::fs::create_dir_all(h.join("projects/old/data")).unwrap();
    std::fs::write(h.join("projects/old/data/x"), vec![0_u8; 2048]).unwrap();
    std::fs::create_dir_all(h.join("ca")).unwrap();
    std::fs::write(h.join("ca/root.crt"), "x").unwrap();
    let rec = serde_json::json!({"stale.test": ["/nowhere"], "elsewhere.test": ["/nowhere"]});
    std::fs::write(h.join("routes.json"), rec.to_string()).unwrap();
    let resolver = tempfile::tempdir().unwrap();
    std::fs::write(
        resolver.path().join("stale.test"),
        "nameserver 127.0.0.1\nport 5323\n",
    )
    .unwrap();
    std::fs::write(
        resolver.path().join("elsewhere.test"),
        "nameserver 10.0.0.1\n",
    )
    .unwrap();
    World {
        _home: home,
        home: h,
        resolver,
    }
}

fn look(w: &World, trusted: bool) -> Machine {
    let mut st = StackView::new(crate::StackKey::new(w.home.join("projects/shop")));
    st.dir = "/work/shop".into();
    st.instances = Arc::new(Vec::new());
    let mut m = gather_with(&w.home, &[st], PS, w.resolver.path(), &|_| trusted);
    m.admin = true;
    m
}

#[test]
fn ps_reads_as_go_read_it() {
    let ps = gather::parse_ps(PS);
    assert_eq!(ps.len(), 4);
    assert_eq!(ps[0].since, "Nov 6 20:00");
    assert_eq!(ps[0].daemon, "/gone/doze.hcl");
    assert_eq!(
        ps[1].exe,
        "/x/.doze/modules/doze/valkey/1.0.0/bin/valkey-plugin"
    );
    assert!(gather::parse_ps("1 2 3").is_empty());
}

#[test]
fn finds_what_no_stack_holds() {
    let w = world();
    let items = find(&look(&w, false));
    let ids: Vec<&str> = items.iter().map(|l| l.id.as_str()).collect();
    // The daemon for a gone doze.hcl; the plugin whose daemon died (ppid 1), not the one still parented, not zsh; the
    // unlisted folder; the route that is doze's, not the one that points elsewhere; the authority nothing uses.
    assert_eq!(ids.len(), 5, "{ids:?}");
    assert_eq!(ids[0], "pid:101");
    assert_eq!(
        items[0].what,
        "a stack's daemon, for a doze.hcl that is gone"
    );
    assert_eq!(ids[1], "pid:102");
    assert_eq!(items[1].what, "valkey-plugin, whose daemon is gone");
    assert!(ids[2].starts_with("dir:") && ids[2].ends_with("projects/old"));
    assert!(items[2].danger && items[2].detail.ends_with("· 2 KB"));
    assert_eq!(ids[3], "route:stale.test");
    assert_eq!(items[3].action, "Remove (one password)");
    assert_eq!(ids[4], "ca:");
    assert_eq!(
        items[4].detail.rsplit(" · ").next(),
        Some("not trusted; its files only")
    );
    assert_eq!(counts(&items), (4, 1));
    assert_eq!(
        summary(&items),
        "5 things stacks left behind, out of step with doze"
    );
    assert_eq!(
        kinds(&items),
        ["daemon", "engine", "state", "route", "authority"]
    );
}

#[test]
fn clean_up_leaves_data_and_asks_once_for_the_routes() {
    let w = world();
    let mut hands = Record::default();
    let done = clear_all(&look(&w, true), &mut hands);
    assert!(!done.bad, "{done:?}");
    assert_eq!(done.said, "cleaned up what was left behind");
    let admin: Vec<&String> = hands
        .asked
        .iter()
        .filter(|a| a.starts_with("admin"))
        .collect();
    // The authority's trust, and every route in one prompt.
    assert_eq!(admin.len(), 2, "{:?}", hands.asked);
    assert!(hands.asked.contains(&"term 101".to_owned()));
    assert!(hands.asked.contains(&"term 102".to_owned()));
    assert!(
        !hands.asked.iter().any(|a| a.contains("projects/old")),
        "data is never cleaned up"
    );
    assert!(!read_routes(&w.home).contains_key("stale.test"));
}

#[test]
fn one_at_a_time_and_only_while_it_is_still_left_behind() {
    let w = world();
    let m = look(&w, false);
    let mut hands = Record::default();
    let old = find(&m).into_iter().find(|l| l.kind == "state").unwrap();
    let done = clear(&old.id, &m, &mut hands);
    assert!(done.said.starts_with("deleted "), "{done:?}");
    // A pid that is not a leftover is never stopped.
    let done = clear("pid:104", &m, &mut hands);
    assert!(done.bad && done.said.contains("no longer left behind"));
    assert!(!hands.asked.iter().any(|a| a.contains("104")));
    // A listed stack's folder is never a leftover.
    let listed = format!("dir:{}", w.home.join("projects/shop").display());
    assert!(clear(&listed, &m, &mut hands).bad);
    // A cancelled prompt removes nothing and says so.
    let mut no = Record {
        cancel: true,
        ..Record::default()
    };
    let done = clear("route:stale.test", &m, &mut no);
    assert_eq!(done.said, "cancelled; nothing was removed");
    assert!(read_routes(&w.home).contains_key("stale.test"));
}

#[test]
fn without_a_prompt_the_machine_parts_say_how() {
    let w = world();
    let mut m = look(&w, true);
    m.admin = false;
    let items = find(&m);
    assert_eq!(
        items.iter().find(|l| l.kind == "route").unwrap().action,
        "Remove"
    );
    let done = clear("route:stale.test", &m, &mut Record::default());
    assert!(done.bad && done.said.contains("doze doctor --fix"));
}
