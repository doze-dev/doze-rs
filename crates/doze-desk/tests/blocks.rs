//! The console's working blocks through the hub, against a fixture daemon on a real socket: a value in parts, a
//! tree page, the daemon's asleep refusal said as it said it, and a form's refusal under the field it names. The
//! requests the daemon saw are checked too: a value is never asked with `wake` unless the press said so, and a form
//! sends every field.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{Home, daemon, until};
use doze_control::fixture::{Answer, Script};
use doze_control::{ChildrenView, InstanceView, Op, Response, TreeNodeView, ValueView, fault};
use doze_desk::page::tree::TreeState;
use doze_desk::page::value::Refusal;
use doze_desk::page::viewer::{Ask, Viewer, Want};
use doze_desk::{BlockAsk, Conn, FormOutcome, Hub};
use std::collections::BTreeMap;

fn answer(op: &str, key: &str, response: Response) -> Answer {
    Answer {
        op: op.into(),
        instance: "db".into(),
        panel: "browse".into(),
        key: key.into(),
        response,
        ..Answer::default()
    }
}

fn script() -> Script {
    let mut part = ValueView {
        media: "json".into(),
        size: 11,
        more: true,
        ..ValueView::default()
    };
    part.set_body(br#"{"a":[1,"#);
    Script {
        status: Response {
            ok: true,
            stack: "shop".into(),
            instances: vec![InstanceView {
                name: "db".into(),
                kind: "module".into(),
                state: "active".into(),
                has_console: true,
                ..InstanceView::default()
            }],
            ..Response::default()
        },
        hold: true,
        answers: vec![
            answer(
                "value",
                "orders",
                Response {
                    value: Some(part),
                    ..Response::default()
                },
            ),
            answer(
                "children",
                "",
                Response {
                    children: Some(ChildrenView {
                        nodes: vec![TreeNodeView {
                            id: "app".into(),
                            label: "app".into(),
                            ..TreeNodeView::default()
                        }],
                        next: "50".into(),
                    }),
                    ..Response::default()
                },
            ),
            answer(
                "value",
                "sleepy",
                Response {
                    error: "db is asleep. Opening a value wakes it.".into(),
                    code: fault::ASLEEP.into(),
                    ..Response::default()
                },
            ),
            Answer {
                panel: String::new(),
                ..answer(
                    "admin",
                    "create",
                    Response {
                        error: "name: a database named app already exists".into(),
                        field: "name".into(),
                        ..Response::default()
                    },
                )
            },
        ],
        ..Script::default()
    }
}

fn want(id: &str) -> Option<Want> {
    Some(Want {
        panel: "browse".into(),
        row: BTreeMap::from([("id".to_owned(), id.to_owned())]),
        ident: id.into(),
        label: id.into(),
    })
}

#[test]
fn values_trees_and_forms_go_through_the_hub() {
    let Some(d) = daemon("blocks", script()) else {
        return;
    };
    let home = Home::new();
    let key = home.stack("shop-1", &d);
    let hub = Hub::start(home.options());
    until(&hub, &key, "live", |s| s.link.conn == Conn::Live);
    let at = BlockAsk {
        instance: "db".into(),
        panel: "browse".into(),
    };

    // The viewer decides each ask, as the app's does; the hub runs it on the fixture daemon.
    let run = |v: &mut Viewer, ask: Option<Ask>| {
        let ask = ask.expect("an ask");
        let got = hub
            .value(&key, &at, ask.row.clone(), ask.offset, ask.wake)
            .map_err(|e| Refusal::of(&e));
        v.answered(&ask, got);
    };
    let mut v = Viewer::default();
    let ask = v.follow(want("orders"), false);
    run(&mut v, ask);
    let doc = v.doc.clone().unwrap();
    assert_eq!(doc.summary(), "JSON · 8 B of 11 B");
    assert!(doc.more && doc.json.is_none(), "a part is not folded yet");
    assert_eq!(
        v.follow(want("orders"), false),
        None,
        "the same pick is not asked again"
    );

    let page = hub.children(&key, &at, "", "", "").unwrap();
    let mut tree = TreeState::default();
    let top = tree.start().unwrap();
    tree.got(&top, page);
    assert_eq!(tree.more("").map(|f| f.cursor), Some("50".to_owned()));

    // A pick on an instance asleep: refused in the daemon's words, until the person presses to open it.
    let ask = v.follow(want("sleepy"), false);
    run(&mut v, ask);
    let r = v.refused.clone().unwrap();
    assert!(r.asleep(), "{r:?}");
    assert_eq!(r.message, "db is asleep. Opening a value wakes it.");
    let ask = v.open_waking();
    run(&mut v, ask);

    let values = BTreeMap::from([
        ("name".to_owned(), "app".to_owned()),
        ("owner".to_owned(), String::new()),
    ]);
    let out = hub
        .admin_form(&key, "db", ("create", ""), values.clone())
        .unwrap();
    assert_eq!(
        out,
        FormOutcome::Refused {
            message: "name: a database named app already exists".into(),
            field: "name".into(),
        }
    );

    let asked = d.requests();
    let values_asked: Vec<bool> = asked
        .iter()
        .filter(|r| r.op == Op::Value.as_str())
        .map(|r| r.wake)
        .collect();
    assert_eq!(
        values_asked,
        [false, false, true],
        "only the press asked to wake"
    );
    let form = asked.iter().find(|r| r.op == Op::Admin.as_str()).unwrap();
    assert!(form.form, "a form says so");
    assert_eq!(form.values, values, "every field, blanks included");
}
