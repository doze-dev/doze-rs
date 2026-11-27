//! The environment that points what the stack runs at the doze authority (Go's `caEnv`). The front door answers every
//! name it fronts over https (`.doze` names as well as declared domains), so an app calling another at
//! `https://<x>.<stack>.doze` must trust the authority too: every stack with names gets it, not only one with a domain.
//! The authority is made here when there is none (files only). `SSL_CERT_FILE` and `REQUESTS_CA_BUNDLE` replace a
//! client's store rather than add to it, so they get the bundle (the machine's roots and the doze root), never the doze
//! root alone, and public https keeps working. A variable the block sets itself wins (`doze_engine::process::
//! with_extra`).

use std::collections::BTreeMap;
use std::path::Path;

/// The variables, or none when names are off or the authority cannot be had (said in the log).
pub(crate) fn ca_env(cfg: &doze_config::Config) -> BTreeMap<String, String> {
    // Names off: nothing is fronted over https. A stack whose domains all bring their own files still has `.doze`
    // names, which the authority serves.
    if !cfg.defaults.names {
        return BTreeMap::new();
    }
    for_home(Path::new(&cfg.home))
}

#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
fn for_home(home: &Path) -> BTreeMap<String, String> {
    let dir = doze_names::ca::ca_dir(home);
    let ca = match doze_names::ca::Ca::load_or_create(&dir) {
        Ok(ca) => ca,
        Err(e) => {
            eprintln!(
                "doze daemon: names: no certificate authority ({e}); the stack's processes will not trust https:// names"
            );
            return BTreeMap::new();
        }
    };
    match ca.ensure_bundle() {
        Ok(bundle) => doze_config::stack::ca_env(&ca.root_path(), &bundle),
        Err(e) => {
            eprintln!("doze daemon: names: could not write the trust bundle: {e}");
            BTreeMap::new()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_with_only_doze_names_trusts_the_authority_through_the_bundle() {
        let home = std::env::temp_dir().join(format!("dz-ca-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let mut cfg = doze_config::Config {
            home: home.display().to_string(),
            instances: vec![doze_config::Instance {
                name: "web".into(),
                port: 3000,
                enabled: true,
                ..doze_config::Instance::default()
            }],
            ..doze_config::Config::default()
        };
        assert!(ca_env(&cfg).is_empty(), "names off: nothing");
        assert!(!home.exists());

        cfg.defaults.names = true;
        let env = ca_env(&cfg);
        let root = home.join("ca/root.crt").display().to_string();
        assert_eq!(env["DOZE_CA_FILE"], root);
        assert_eq!(env["NODE_EXTRA_CA_CERTS"], root, "node adds to its store");
        let bundle = &env["SSL_CERT_FILE"];
        assert_ne!(bundle, &root, "never the doze root alone");
        assert_eq!(&env["REQUESTS_CA_BUNDLE"], bundle);
        let text = std::fs::read_to_string(bundle).unwrap();
        let ours = std::fs::read_to_string(&root).unwrap();
        assert!(text.contains(ours.trim()), "the bundle holds the doze root");
        if cfg!(target_os = "macos") {
            assert!(
                text.matches("-----BEGIN CERTIFICATE-----").count() > 1,
                "and the machine's own roots"
            );
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_variable_the_block_sets_keeps_its_value() {
        let mut env = BTreeMap::from([(
            "REQUESTS_CA_BUNDLE".to_owned(),
            "/corp/bundle.pem".to_owned(),
        )]);
        let extra = BTreeMap::from([
            (
                "REQUESTS_CA_BUNDLE".to_owned(),
                "/doze/bundle.pem".to_owned(),
            ),
            ("SSL_CERT_FILE".to_owned(), "/doze/bundle.pem".to_owned()),
        ]);
        doze_engine::process::with_extra(&mut env, &extra);
        assert_eq!(env["REQUESTS_CA_BUNDLE"], "/corp/bundle.pem");
        assert_eq!(env["SSL_CERT_FILE"], "/doze/bundle.pem");
    }
}
