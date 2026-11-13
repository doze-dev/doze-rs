use super::*;
use crate::overview::Decl;

fn d(name: &str, port: Option<u32>, needs: &[&str]) -> Decl {
    Decl {
        name: name.into(),
        kind: "app".into(),
        enabled: true,
        port,
        needs: needs.iter().map(|s| (*s).to_owned()).collect(),
        ..Decl::default()
    }
}

fn acme() -> (tempfile::TempDir, Shape) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("doze.hcl"), "stack = \"acme\"\n").unwrap();
    let mut web = d("web", Some(3000), &["api"]);
    web.domains = vec!["dev.acme.com".into()];
    let shape = Shape {
        stack: "acme".into(),
        decls: vec![
            d("api", Some(8080), &["db"]),
            web,
            d("worker", None, &[]),
            Decl {
                kind: "module".into(),
                ..d("db", Some(5432), &[])
            },
        ],
        ..Shape::default()
    };
    (dir, shape)
}

fn held(port: u16, pid: i64, cmd: &str) -> Holder {
    Holder {
        port,
        pid,
        command: cmd.into(),
        doze: false,
        stack: String::new(),
        service: String::new(),
    }
}

#[test]
fn nothing_in_the_way_says_what_starts_and_what_is_free() {
    let (dir, shape) = acme();
    let p = preflight(
        "acme",
        dir.path(),
        &shape,
        &[],
        ("up-last", &[]),
        &Looked::default(),
    );
    assert!(!p.blocked);
    let titles: Vec<&str> = p.checks.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "The folder is there, and doze.hcl loads",
            "Ports 3000, 5432 and 8080 are free"
        ]
    );
    assert!(
        p.checks
            .first()
            .is_some_and(|c| c.detail.ends_with("starts api, db, web and worker"))
    );
    let web = preflight(
        "acme",
        dir.path(),
        &shape,
        &["web".into()],
        ("up-last", &[]),
        &Looked::default(),
    );
    assert!(
        web.checks
            .first()
            .is_some_and(|c| c.detail.ends_with("starts web, and what they need"))
    );
}

#[test]
fn a_port_held_by_a_stranger_offers_to_stop_it_or_start_without() {
    let (dir, shape) = acme();
    let mut looked = Looked::default();
    looked.held.insert(3000, held(3000, 4242, "node"));
    looked.untrusted = vec!["dev.acme.com".into()];
    let p = preflight("acme", dir.path(), &shape, &[], ("up-last", &[]), &looked);
    assert!(p.blocked);
    let c = p.checks.get(1).unwrap();
    assert_eq!(c.title, "Port 3000 for web is taken");
    assert_eq!((c.detail.as_str(), c.mono), ("node (pid 4242)", true));
    let fixes: Vec<&str> = c.fixes.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(
        fixes,
        ["Stop pid 4242, then Up", "Up without web", "Edit doze.hcl"]
    );
    assert_eq!(c.fixes.first().map(|f| &f.kind), Some(&FixKind::Stop(4242)));
    let warn = p.checks.iter().find(|c| c.tone == Tone::Warn).unwrap();
    assert_eq!(warn.title, "dev.acme.com is not set up on this Mac");
    assert_eq!(
        p.checks.last().map(|c| c.title.as_str()),
        Some("Ports 5432 and 8080 are free")
    );
}

#[test]
fn a_needed_service_cannot_be_left_out_and_another_stack_is_never_stopped() {
    let (dir, shape) = acme();
    let mut looked = Looked::default();
    let mut h = held(8080, 77, "api");
    (h.doze, h.stack, h.service) = (true, "shop".into(), "api".into());
    looked.held.insert(8080, h);
    looked.unbindable.insert(5432);
    let p = preflight("acme", dir.path(), &shape, &[], ("up-last", &[]), &looked);
    let api = p.checks.iter().find(|c| c.title.contains("8080")).unwrap();
    assert_eq!(api.title, "Port 8080 for api is used by api in shop");
    assert!(
        api.fixes
            .iter()
            .all(|f| !matches!(f.kind, FixKind::Stop(_) | FixKind::Without(_))),
        "web needs api"
    );
    let db = p.checks.iter().find(|c| c.title.contains("5432")).unwrap();
    assert_eq!(
        db.detail,
        "by a process of another user, which doze cannot see or stop"
    );
}

#[test]
fn a_gone_folder_and_a_broken_file_block_before_anything_else() {
    let (dir, mut shape) = acme();
    let gone = dir.path().join("nope");
    let p = preflight(
        "acme",
        &gone,
        &shape,
        &[],
        ("up-last", &[]),
        &Looked::default(),
    );
    assert_eq!(
        p.checks.first().map(|c| c.title.as_str()),
        Some("acme's folder is gone")
    );
    assert_eq!(
        p.checks
            .first()
            .and_then(|c| c.fixes.first())
            .map(|f| &f.kind),
        Some(&FixKind::Remove)
    );
    shape.error = Some("Error: Unsupported block type\n\n  on doze.hcl line 1:".into());
    let p = preflight(
        "acme",
        dir.path(),
        &shape,
        &[],
        ("up-last", &[]),
        &Looked::default(),
    );
    let c = p.checks.first().unwrap();
    assert!(p.blocked);
    assert_eq!(c.title, "doze.hcl has a mistake");
    assert_eq!(c.detail, "doze.hcl line 1: Unsupported block type");
}

// Go: TestPreflightInstance.
#[test]
fn one_service_asks_only_when_its_port_is_held() {
    let (dir, shape) = acme();
    let mut looked = Looked::default();
    looked.held.insert(8080, held(8080, 9, "python3"));
    let p = preflight_instance("api", false, false, dir.path(), &shape, &looked);
    assert!(p.blocked);
    let fixes: Vec<&str> = p
        .checks
        .iter()
        .flat_map(|c| &c.fixes)
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(fixes, ["Stop pid 9, then start api", "Edit doze.hcl"]);
    assert!(
        !preflight_instance("api", true, false, dir.path(), &shape, &looked).blocked,
        "awake: nothing to say"
    );
    assert!(
        preflight_instance("web", false, false, dir.path(), &shape, &looked)
            .checks
            .is_empty()
    );
}
