//! What the postgres acceptance tests share: the real build, the block on its major, psql on the instance's socket,
//! and a re-converge that prunes what left the block.
#![allow(dead_code)]

use doze_module_proto::msg::calls::PruneArgs;
use doze_module_proto::msg::instance::Address;
use doze_module_proto::msg::run::WithToolchain;
use doze_module_test::{Booted, Harness, acceptance};
use doze_os::tree::Stopped;

pub(crate) fn harness() -> Option<Harness> {
    acceptance(env!("CARGO_BIN_EXE_doze-module-postgres"), "postgres").unwrap()
}

/// The build's major ("17" from "postgres (PostgreSQL) 17.9"): every block declares it, as the data must match.
pub(crate) fn major(h: &Harness) -> String {
    let out = std::process::Command::new(h.bin_dir.join("postgres"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .last()
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .to_string()
}

/// The block with the build's major in place of `version = 17`.
pub(crate) fn on_build(h: &Harness, hcl: &str) -> String {
    hcl.replace("version = 17", &format!("version = {}", major(h)))
}

/// The socket's directory and port.
pub(crate) fn socket(b: &Booted) -> (String, String) {
    let Some(Address::Unix(sock)) = b.backend() else {
        panic!("a socket backend, got {:?}", b.backend())
    };
    let p = std::path::Path::new(&sock);
    let port = p
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .strip_prefix(".s.PGSQL.")
        .unwrap();
    (
        p.parent().unwrap().to_string_lossy().into_owned(),
        port.to_string(),
    )
}

/// `psql -tA -c q` as `user` on `db` over the instance's socket: its output, or its error.
pub(crate) fn sql(b: &Booted, user: &str, db: &str, q: &str) -> Result<String, String> {
    let (dir, port) = socket(b);
    let bin = std::path::Path::new(&b.toolchain.bin_dir).join("psql");
    let out = std::process::Command::new(bin)
        .args([
            "-X",
            "-h",
            &dir,
            "-p",
            &port,
            "-U",
            user,
            "-d",
            db,
            "-v",
            "ON_ERROR_STOP=1",
            "-tAc",
            q,
        ])
        .output()
        .unwrap();
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub(crate) fn graceful(stops: &doze_module_test::Stops) {
    assert!(
        !stops.is_empty()
            && stops
                .iter()
                .all(|(_, r)| matches!(r, Ok(Stopped::Graceful(st)) if st.success())),
        "{stops:?}"
    );
}

pub(crate) async fn reconverge(
    b: &Booted,
    hcl: &str,
) -> Vec<doze_module_proto::msg::instance::Object> {
    let before = b.module.objects(&b.instance).await.unwrap();
    let parsed = doze_module_test::hcl::parse_block(hcl, &b.module.manifest.schema).unwrap();
    let mut inst = b.instance.clone();
    inst.spec = parsed.spec;
    b.module.decode(&inst).await.unwrap();
    let with = WithToolchain {
        instance: inst.clone(),
        toolchain: b.toolchain.clone(),
    };
    b.module.converge(&with).await.unwrap();
    let after = b.module.objects(&inst).await.unwrap();
    let removed: Vec<_> = before
        .into_iter()
        .filter(|o| !after.iter().any(|a| a.kind == o.kind && a.name == o.name))
        .collect();
    let args = PruneArgs {
        instance: inst,
        toolchain: b.toolchain.clone(),
        removed: removed.clone(),
    };
    b.module.prune(&args).await.unwrap();
    removed
}
