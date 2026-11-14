//! Go's `engine/process` tests that are about the engine (decoding is doze-config's).

use super::*;

fn ctx(dir: &Path) -> PlanCtx {
    PlanCtx {
        name: "api".into(),
        data_dir: dir.join("clusters/api"),
        run_dir: dir.join("run/api"),
        needs: BTreeMap::new(),
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-engine-proc-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn env_precedence_is_env_over_env_file_and_doze_says_where_to_listen() {
    let d = tmp("env");
    let f = d.join(".env");
    std::fs::write(
        &f,
        "# c\nexport A=file\nB=\"quoted\"\nC='single'\nD=x=y\n\nnoequals\n",
    )
    .unwrap();
    let mut spec = ProcessSpec::new(["x"], &d).env("A", "block").port(3000);
    spec.env_file = Some(f);
    let env = spec.env_for(&ctx(&d), Some(41234));
    assert_eq!(env.get("A").map(String::as_str), Some("block"));
    assert_eq!(env.get("B").map(String::as_str), Some("quoted"));
    assert_eq!(env.get("C").map(String::as_str), Some("single"));
    assert_eq!(env.get("D").map(String::as_str), Some("x=y"));
    assert_eq!(env.get("PORT").map(String::as_str), Some("41234"));
    assert_eq!(env.get("HOST").map(String::as_str), Some(PRIVATE_HOST));
    assert!(env.get("DATA").is_some_and(|v| v.ends_with("api")));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_fronted_app_listens_on_its_private_port_and_a_direct_one_on_its_own() {
    let d = tmp("ports");
    let fronted = ProcessSpec::new(["x"], &d).port(3000);
    let p = fronted.planned(&ctx(&d), Some(45000)).unwrap();
    assert_eq!(p.endpoint.backend, "127.0.0.1:45000");
    let proc0 = p.plan.processes.first().cloned().unwrap();
    assert_eq!(proc0.ready, Some(Ready::Tcp("127.0.0.1:45000".into())));
    assert_eq!(proc0.stop, Some(StopHow::Terminate));
    let mut direct = ProcessSpec::new(["x"], &d).port(3000);
    direct.direct = true;
    let p = direct.planned(&ctx(&d), Some(45000)).unwrap();
    assert_eq!(p.endpoint.backend, "127.0.0.1:3000");
    let env = &p.plan.processes.first().unwrap().env;
    assert!(
        !env.contains_key("PORT"),
        "a direct app already knows its port"
    );
    let worker = ProcessSpec::new(["x"], &d);
    let p = worker.planned(&ctx(&d), None).unwrap();
    assert_eq!(p.endpoint.backend, "");
    assert_eq!(
        p.plan.processes.first().unwrap().ready,
        None,
        "a worker gets the liveness check"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn checks_translate_with_the_port() {
    let d = tmp("ready");
    let spec = |p| ProcessSpec::new(["x"], &d).port(3000).ready(p);
    let r = |s: ProcessSpec| {
        s.planned(&ctx(&d), Some(5000))
            .unwrap()
            .plan
            .processes
            .first()
            .unwrap()
            .ready
            .clone()
    };
    assert_eq!(
        r(spec(Probe::Http("/up".into()))),
        Some(Ready::Http("http://127.0.0.1:5000/up".into()))
    );
    assert_eq!(
        r(spec(Probe::Log("ready (v1.2)".into()))),
        Some(Ready::LogLine(r"ready \(v1\.2\)".into()))
    );
    assert_eq!(
        r(spec(Probe::Exec(vec!["true".into()]))),
        Some(Ready::Exec(vec!["true".into()]))
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_missing_dir_or_env_file_is_said_before_the_start() {
    let d = tmp("missing");
    let gone = ProcessSpec::new(["x"], d.join("nope"));
    let e = gone.planned(&ctx(&d), None).err().unwrap();
    assert!(
        e.starts_with("app \"api\": dir \"") && e.ends_with("does not exist"),
        "{e}"
    );
    let mut spec = ProcessSpec::new(["x"], &d);
    spec.env_file = Some(d.join(".env.missing"));
    let e = spec.planned(&ctx(&d), None).err().unwrap();
    assert!(
        e.contains("env_file") && e.contains("copying .env.example"),
        "{e}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_fronted_app_sleeps_by_default_and_sleep_decides_last() {
    assert_eq!(idle_policy(true, true, Sleep::Default), (false, None));
    assert_eq!(idle_policy(true, false, Sleep::Default), (true, None));
    assert_eq!(idle_policy(false, false, Sleep::Default), (false, None));
    let d = Duration::from_secs(30);
    assert_eq!(idle_policy(true, false, Sleep::After(d)), (false, Some(d)));
    assert_eq!(idle_policy(false, false, Sleep::Never), (true, None));
}

/// Go's `TestAProxiedAppMayNotSetItsOwnPort`: a fronted app may not set `PORT` or `HOST`; a portless one may.
#[test]
fn a_fronted_app_may_not_choose_its_own_port() {
    let d = tmp("ownport");
    for name in ["PORT", "HOST"] {
        let err = ProcessSpec::new(["x"], &d)
            .env(name, "8080")
            .port(8080)
            .check_block(Sleep::Default)
            .unwrap_err();
        assert!(
            err.contains(&format!("env sets {name}")) && err.contains("direct = true"),
            "{err}"
        );
    }
    let worker = ProcessSpec::new(["x"], &d).env("PORT", "9");
    assert!(
        worker.check_block(Sleep::Default).is_ok(),
        "a portless app may set PORT"
    );
    let mut direct = ProcessSpec::new(["x"], &d).env("PORT", "9").port(8080);
    direct.direct = true;
    assert!(
        direct.check_block(Sleep::Default).is_ok(),
        "a direct app binds it itself"
    );
}

/// Go's `TestSleepNeedsDozeToHoldThePort`: only a fronted app can sleep, since only a connection wakes it.
#[test]
fn only_a_fronted_app_may_sleep() {
    let d = tmp("sleepport");
    let ten = Sleep::After(Duration::from_secs(600));
    assert!(
        ProcessSpec::new(["x"], &d)
            .port(8080)
            .check_block(ten)
            .is_ok()
    );
    let err = ProcessSpec::new(["x"], &d).check_block(ten).unwrap_err();
    assert_eq!(
        err,
        "sleep = \"10m0s\" needs doze to hold the app's port, so that a connection can wake it: declare a `port`, and do not set `direct`"
    );
    let mut direct = ProcessSpec::new(["x"], &d).port(8080);
    direct.direct = true;
    direct.service = true;
    let err = direct.check_block(ten).unwrap_err();
    assert!(err.contains("the service's port"), "{err}");
    assert!(
        ProcessSpec::new(["x"], &d)
            .check_block(Sleep::Never)
            .is_ok()
    );
    // Go's third `BindPort` refusal: a port check with no port (audit 3, minor 9).
    let err = ProcessSpec::new(["x"], &d)
        .ready(Probe::Tcp)
        .check_block(Sleep::Default)
        .unwrap_err();
    assert_eq!(err, "ready = tcp(…) needs a `port` to probe");
    // The engine refuses such a block when it is built.
    let svc = crate::Service::process("w", ProcessSpec::new(["x"], &d)).sleep(ten);
    let built = crate::Engine::builder("t", &d).service(svc).build();
    let err = built.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err.starts_with("\"w\": sleep = "), "{err}");
}

/// Go's `TestAProcessesOwnVariableWinsOverDozesAndAnInheritedOneDoesNot`: doze's variables replace what a process
/// merely inherited and yield to what it was given on purpose.
#[test]
fn a_chosen_variable_wins_over_dozes_and_an_inherited_one_does_not() {
    let inherited = std::env::var("PATH").unwrap();
    let extra: BTreeMap<String, String> = [
        ("PATH", "/x/bin"),
        ("REQUESTS_CA_BUNDLE", "/x/bundle.pem"),
        ("DOZE_TEST_CA_FILE", "/x/root.crt"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect();
    let mut env: BTreeMap<String, String> = [
        ("A", "1"),
        ("PATH", inherited.as_str()),
        ("REQUESTS_CA_BUNDLE", "/corp/bundle.pem"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect();
    with_extra(&mut env, &extra);
    let got: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        got,
        [
            ("A", "1"),
            ("DOZE_TEST_CA_FILE", "/x/root.crt"),
            ("PATH", "/x/bin"),
            ("REQUESTS_CA_BUNDLE", "/corp/bundle.pem"),
        ]
    );
}
