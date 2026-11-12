//! temporal on the real dev server (`DOZE_TEMPORAL_BINDIR`, from doze-binaries). Each test is a promise the module
//! makes, checked on the engine: the published example is ready with its namespaces and search attribute; two of it
//! run side by side; converge changes only what differs and never mistakes a dead server for a missing namespace; a
//! restart keeps the data; the app sees the namespaces; a mistake is refused before boot, and only what temporal
//! itself refuses. Portable: temporal ships on macOS, Linux and Windows.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
use std::process::Command;
use std::time::Duration;

fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-temporal"), "temporal").unwrap()
}

fn cli(h: &Harness) -> std::path::PathBuf {
    h.bin_dir
        .join(format!("temporal{}", std::env::consts::EXE_SUFFIX))
}

fn frontend(b: &Booted) -> String {
    match &b.instance.listeners["main"].backend {
        Address::Tcp(a) => a.clone(),
        other => panic!("temporal must be granted TCP, got {other:?}"),
    }
}

/// `temporal <args> --address <frontend>`: stdout, or the failure's output.
fn run(h: &Harness, b: &Booted, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cli(h))
        .args(args)
        .args(["--address", &frontend(b)])
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

fn retention(h: &Harness, b: &Booted, ns: &str) -> String {
    let out = run(
        h,
        b,
        &["operator", "namespace", "describe", "-n", ns, "-o", "json"],
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    v["config"]["workflowExecutionRetentionTtl"]
        .as_str()
        .unwrap()
        .to_string()
}

fn with(b: &Booted) -> WithToolchain {
    WithToolchain {
        instance: b.instance.clone(),
        toolchain: b.toolchain.clone(),
    }
}

async fn stop(b: Booted) {
    let stops = b.stop(Duration::from_secs(20)).await;
    assert!(stops.iter().all(|(_, r)| r.is_ok()), "{stops:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn the_example_is_ready_with_its_namespaces_and_search_attribute() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    // Ready means SERVING, and the namespaces are there at that moment, before any converge.
    assert_eq!(retention(&h, &b, "orders"), "604800s", "7d");
    assert!(
        run(
            &h,
            &b,
            &["operator", "namespace", "describe", "-n", "billing"]
        )
        .is_ok()
    );
    let attrs = run(&h, &b, &["operator", "search-attribute", "list"]).unwrap();
    assert!(
        attrs.contains("CustomerId") && attrs.contains("Keyword"),
        "{attrs}"
    );
    b.module.health(&b.instance).await.unwrap();
    let ui = b.instance.listeners.get("ui").expect("a Web UI listener");
    assert!(matches!(ui.backend, Address::Tcp(_)), "{ui:?}");
    stop(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn two_of_them_run_side_by_side() {
    let Some(h) = harness() else { return };
    // Both declare 7233 and 8233; Go bound those itself and the second failed. The host grants each its own.
    let (a, b) = tokio::join!(h.boot_example(), h.boot_example());
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(frontend(&a), frontend(&b));
    a.module.health(&a.instance).await.unwrap();
    b.module.health(&b.instance).await.unwrap();
    stop(a).await;
    stop(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn converge_changes_what_differs_and_a_dead_server_is_not_a_missing_namespace() {
    let Some(h) = harness() else { return };
    let mut b = h.boot_example().await.unwrap();
    let changed = "temporal \"dev\" {\n  version = 1\n  namespace \"orders\" {\n    retention = \"3d\"\n  }\n  namespace \"audit\" {\n    description = \"who did what\"\n  }\n}\n";
    b.instance.spec = doze_module_test::hcl::parse_block(changed, &b.module.manifest.schema)
        .unwrap()
        .spec;
    b.module.converge(&with(&b)).await.unwrap();
    assert_eq!(retention(&h, &b, "orders"), "259200s", "3d");
    let audit = run(
        &h,
        &b,
        &["operator", "namespace", "describe", "-n", "audit"],
    )
    .unwrap();
    assert!(audit.contains("who did what"), "{audit}");
    // Converging again is a no-op, not an error.
    b.module.converge(&with(&b)).await.unwrap();
    // Server gone: an error, not an attempt to create what merely cannot be seen (module bug 9).
    let (module, instance, toolchain) = (b.module.clone(), b.instance.clone(), b.toolchain.clone());
    stop(b).await;
    let e = module
        .converge(&WithToolchain {
            instance,
            toolchain,
        })
        .await
        .unwrap_err();
    assert!(!e.message.contains("already"), "{e:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_restart_keeps_its_data_and_the_app_sees_the_namespaces() {
    let Some(h) = harness() else { return };
    let b = h.boot_example().await.unwrap();
    run(&h, &b, &["operator", "namespace", "create", "-n", "kept"]).unwrap();
    let (_, b) = b.sleep_and_wake(Duration::from_secs(20)).await.unwrap();
    assert!(!b.fresh);
    assert!(
        run(&h, &b, &["operator", "namespace", "describe", "-n", "kept"]).is_ok(),
        "SQLite kept it"
    );
    let res = b.module.resources(&b.instance).await.unwrap();
    let orders = res
        .iter()
        .find(|r| r.name == "orders")
        .expect("orders listed");
    assert_eq!(
        (
            orders.info["retention"].as_str(),
            orders.info["running"].as_str()
        ),
        ("7 days", "0")
    );
    assert_eq!(orders.status, "registered");
    assert!(
        !res.iter().any(|r| r.name == "temporal-system"),
        "temporal's own namespace is not the app's business"
    );
    stop(b).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_mistake_is_refused_before_boot_and_only_what_temporal_refuses() {
    let Some(h) = harness() else { return };
    for (hcl, says) in [
        (
            "namespace \"o\" {\n    retention = \"seven days\"\n  }",
            "7d, 168h",
        ),
        (
            "search_attributes = { Region = \"Keywrd\" }",
            "did you mean \"Keyword\"",
        ),
        (
            "restart {\n    policy = \"on-failure\"\n  }",
            "did you mean \"on_failure\"",
        ),
    ] {
        let e = h
            .boot(&format!("temporal \"r\" {{\n  version = 1\n  {hcl}\n}}\n"))
            .await
            .unwrap_err();
        assert!(e.contains(says), "{hcl:?}: {e}");
    }
    // temporal reads a type without regard to case; the module must not refuse what it accepts.
    let b = h
        .boot(
            "temporal \"case\" {\n  version = 1\n  search_attributes = { Plan = \"keyword\" }\n}\n",
        )
        .await
        .unwrap();
    assert!(
        run(&h, &b, &["operator", "search-attribute", "list"])
            .unwrap()
            .contains("Plan")
    );
    stop(b).await;
    // And what the module refuses, temporal refuses too.
    let out = Command::new(cli(&h))
        .args([
            "server",
            "start-dev",
            "--headless",
            "--port",
            "0",
            "--search-attribute",
            "X=Keywrd",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid search attribute value type"));
}
