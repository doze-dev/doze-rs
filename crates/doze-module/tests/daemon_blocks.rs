//! The daemon's console-block ops with the echo module (`value`, `children`, and `admin` with a form's values): what
//! the wire carries is what the module answers, a sleeping instance wakes only when the request says the person opened
//! a value, and nothing in the daemon names the module.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod daemon_support;

use daemon_support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_blocks_are_asked_of_the_module_and_wake_only_on_a_press() {
    let (dir, _port) = project("blocks");
    let Some((d, s, sock, _front)) = serve(&dir).await else {
        return;
    };
    let mut c = tokio::task::spawn_blocking(move || Raw::open(&sock))
        .await
        .unwrap();
    let asleep = |r: &serde_json::Value| r["ok"] == false && r["code"] == "asleep";

    // Asleep: nothing is asked of it until the person opens a value, and a tree never wakes it.
    let r = c.ask(r#"{"id":1,"op":"value","db":"e","panel":"messages","row":{"name":"note"}}"#);
    assert!(asleep(&r), "{r}");
    assert_eq!(r["error"], "e is asleep. Opening a value wakes it.");
    let r = c.ask(r#"{"id":2,"op":"children","db":"e","panel":"browse"}"#);
    assert!(asleep(&r), "{r}");
    assert!(!d.engine().get("e").unwrap().state.running());

    let r = c.ask(
        r#"{"id":3,"op":"value","db":"e","panel":"messages","row":{"name":"note"},"wake":true}"#,
    );
    assert_eq!(r["ok"], true, "{r}");
    assert!(
        d.engine().get("e").unwrap().state.running(),
        "opening it woke it"
    );
    assert_eq!(r["value"]["media"], "text", "{r}");
    assert_eq!(
        r["value"]["base64"],
        serde_json::Value::Null,
        "text travels as text"
    );

    values_trees_and_forms(&mut c);

    // An instance whose kind has no such block says so.
    let r = c.ask(r#"{"id":13,"op":"value","db":"show","panel":"m","wake":true}"#);
    assert_eq!(
        r["error"], "instance \"show\" (process) has no value viewer",
        "{r}"
    );
    let r = c.ask(r#"{"id":14,"op":"value","db":"nope","panel":"m"}"#);
    assert_eq!(r["code"], "not_found", "{r}");

    s.stop();
    d.engine().shutdown().await;
}

/// A value in parts, a tree page, and a form's values with the field a refusal is about.
fn values_trees_and_forms(c: &mut Raw) {
    let r = c.ask(r#"{"id":7,"op":"value","db":"e","panel":"messages","row":{"name":"blob"}}"#);
    let v = &r["value"];
    assert_eq!(
        (v["media"].as_str(), v["base64"].as_bool()),
        (Some("bytes"), Some(true)),
        "{r}"
    );
    let r = c.ask(r#"{"id":8,"op":"value","db":"e","panel":"messages","row":{"name":"order-10433"},"offset":13}"#);
    assert_eq!(r["value"]["offset"], 13, "{r}");
    assert!(
        r["value"]["body"].as_str().unwrap().starts_with("status"),
        "{r}"
    );

    let r = c.ask(r#"{"id":9,"op":"children","db":"e","panel":"browse","parent":"app/audit"}"#);
    assert_eq!(r["children"]["nodes"].as_array().unwrap().len(), 50, "{r}");
    assert_eq!(r["children"]["next"], "50");

    // A form: the field a refusal is about comes back for the form to mark.
    let r = c.ask(
        r#"{"id":10,"op":"admin","db":"e","action":"create","values":{"name":"p","size":"5000"}}"#,
    );
    assert_eq!(
        (r["ok"].as_bool(), r["field"].as_str()),
        (Some(false), Some("size")),
        "{r}"
    );
    let r = c.ask(r#"{"id":11,"op":"admin","db":"e","action":"create","values":{"name":"p"}}"#);
    assert_eq!(
        r["result"], "created channel p (size 1, delete, kept in doze.hcl, 0 headers)",
        "{r}"
    );
    let r = c.ask(r#"{"id":12,"op":"resources","db":"e"}"#);
    let create = r["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "create")
        .unwrap()
        .clone();
    assert_eq!(create["fields"][1]["min"], 1, "{create}");
    assert_eq!(create["input_hint"], "name");
}
