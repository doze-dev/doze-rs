//! Client TLS at the front (Go's `proxy.BuildServerTLS` and the `tls { }` block): the certificate a module that takes
//! its own connections (`wire.accept`, the postgres filter) terminates its clients' TLS with, and whether a plain
//! client is refused. The proxy-to-engine hop stays plain.
//!
//! The person's `cert` and `key` files when the block names them; else a certificate minted from the machine's doze
//! authority (Go made a self-signed one per project, which no client could verify): for `*.<stack>.doze` with names
//! on, for `localhost` without. `doze trust` puts the authority in the machine's store, so `sslmode=verify-full`
//! passes.

use std::path::Path;

/// What a module terminating TLS is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireTls {
    /// The certificate and key, DER.
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    /// A plain client is refused.
    pub required: bool,
}

/// The stack's TLS, if its doze.hcl has a `tls` block; an error says why it cannot be had (Go refused to start).
pub fn for_config(cfg: &doze_config::Config) -> Result<Option<WireTls>, String> {
    let t = &cfg.tls;
    if !t.enabled {
        return Ok(None);
    }
    let resolve = |p: &str| {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            Path::new(&cfg.dir).join(p)
        }
    };
    let (cert, key) = if t.cert.is_empty() {
        let dir = doze_names::ca::ca_dir(Path::new(&cfg.home));
        let ca = doze_names::ca::Ca::load_or_create(&dir)
            .map_err(|e| format!("configuring TLS: {e}"))?;
        let host = if cfg.defaults.names {
            format!(
                "*.{}.{}",
                doze_names::label(&cfg.stack()),
                doze_names::SUFFIX
            )
        } else {
            "localhost".to_owned()
        };
        let leaf = ca
            .leaf(&host)
            .map_err(|e| format!("configuring TLS: {e}"))?;
        (leaf.cert_der.clone(), leaf.key_der.clone())
    } else {
        doze_names::certs::pem_pair_der(&resolve(&t.cert), &resolve(&t.key))
            .map_err(|e| format!("configuring TLS: loading the keypair: {e}"))?
    };
    Ok(Some(WireTls {
        cert,
        key,
        required: t.required,
    }))
}
