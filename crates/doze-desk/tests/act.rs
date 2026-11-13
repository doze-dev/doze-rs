//! The confirmations against the Go app's goldens (`internal/desk/view/testdata`, doze 23b73d6: `shop-db-sleep`,
//! `shop-db-restart`, `shop-db-reset`, `modules-events-produce`), field for field; and a stack action through a real
//! CLI subprocess (a stand-in `doze` script that answers with the CLI's envelope).
#![allow(clippy::unwrap_used, clippy::panic)]

#[path = "common/mod.rs"]
mod common;

use std::path::Path;
use std::sync::Arc;

use doze_desk::act::{self, Cli, StackAction};
use doze_desk::page::console::Action;
use doze_desk::{StackKey, StackView};
use serde_json::Value;

fn golden(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("testdata/go/view/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn shop() -> StackView {
    let s = common::script("shop");
    let mut st = StackView::new(StackKey::new("/x/projects/shop-1"));
    st.name = "shop".into();
    st.running = true;
    st.instances = Arc::new(s.status.instances);
    st
}

#[test]
fn the_dialogs_say_what_go_says() {
    let st = shop();
    for (verb, file) in [
        ("sleep", "shop-db-sleep"),
        ("restart", "shop-db-restart"),
        ("reset", "shop-db-reset"),
    ] {
        let d = act::dialog_of(&st, "db", verb, None);
        assert_eq!(serde_json::to_value(&d).unwrap(), golden(file), "{verb}");
    }
    assert!(
        !act::dialog_of(&st, "web", "sleep", None).needed,
        "nothing uses web; sleeping it never asks"
    );
    let stop = act::dialog_of(&st, "", "down", None);
    assert!(stop.needed && stop.stack);
    assert_eq!(stop.title, "Stop shop?");
    let remove = act::dialog_of(&st, "", "remove", None);
    assert_eq!((remove.needed, remove.default), (true, 1));
}

#[test]
fn a_module_action_asks_for_its_input_as_go_does() {
    let produce = Action {
        id: "produce".into(),
        label: "Produce a message".into(),
        key: 'P',
        destructive: false,
        hint: "message value".into(),
        resource: "orders".into(),
    };
    let d = act::action_dialog("events", &produce);
    assert_eq!(serde_json::to_value(&d).unwrap(), golden("modules-events-produce"));
    let keep = Action {
        hint: String::new(),
        ..produce
    };
    assert!(!act::action_dialog("events", &keep).needed, "it takes no input and deletes nothing: it just runs");
}

/// A stand-in `doze` that fails `up` as the CLI does (its progress on stderr, one envelope on stdout) and does `down`.
fn fake_doze(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("doze");
    std::fs::write(
        &p,
        "#!/bin/sh\necho \"$*\" >> \"$DOZE_HOME/calls\"\ncase \"$*\" in\n  *down*) echo '{\"ok\":true}'; exit 0;;\nesac\necho 'starting db…' >&2\necho '{\"schema\":\"doze/v1\",\"ok\":false,\"error\":{\"code\":\"failed\",\"message\":\"port 5432 is taken\",\"exit\":1}}'\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    p
}

#[cfg(unix)]
#[test]
fn a_stack_action_runs_the_cli_in_the_folder_and_reads_its_failure() {
    let (bin, home, work) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let cli = Cli {
        program: fake_doze(bin.path()),
        home: home.path().to_path_buf(),
    };
    assert!(cli.stack(&StackAction::Down, "shop", work.path()).is_ok());
    let f = cli
        .stack(&StackAction::Restart, "shop", work.path())
        .unwrap_err();
    assert_eq!(f.what, "Couldn't restart shop");
    assert_eq!(f.problem, "port 5432 is taken");
    assert_eq!(f.lines, ["starting db…", "Error: port 5432 is taken"]);
    let calls = std::fs::read_to_string(home.path().join("calls")).unwrap();
    assert_eq!(
        calls,
        "--json -y down\n--json -y down\n--json -y up --last\n",
        "restart is down, then up as last time, in the doze home the app shows"
    );
}
