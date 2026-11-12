//! The TLS flags: `--tls-cert` and `--tls-key`, both or neither, checked in the Go edition's words.
//!
//! In a build without the `tls` feature they are refused, naming the feature: a broker that ignored them would
//! send in the clear.

use anyhow::{Result, bail};

/// What the flags come to: the certificate and key loaded, or (without the feature) nothing.
#[cfg(feature = "tls")]
pub(super) type Loaded = Option<kafka_server::TlsConfig>;
#[cfg(not(feature = "tls"))]
pub(super) type Loaded = ();

/// Checks the two flags and, in a build with TLS, loads the files: a certificate or key that cannot be used stops the
/// start here, before anything is bound.
#[cfg(feature = "tls")]
pub(super) fn load(
    cert: Option<&std::path::PathBuf>,
    key: Option<&std::path::PathBuf>,
) -> Result<Loaded> {
    let (cert, key) = match (cert, key) {
        (None, None) => return Ok(None),
        (Some(cert), Some(key)) => (cert, key),
        _ => bail!("config: TLS needs both --tls-cert and --tls-key, or neither"),
    };
    let loaded = kafka_server::TlsConfig::load(cert, key).map_err(|e| {
        anyhow::anyhow!(
            "config: cannot load the TLS certificate and key (--tls-cert {}, --tls-key {}): {e}",
            cert.display(),
            key.display()
        )
    })?;
    Ok(Some(loaded))
}

/// Without the feature the flags are refused, whatever they hold.
#[cfg(not(feature = "tls"))]
pub(super) fn load(cert: Option<&String>, key: Option<&String>) -> Result<Loaded> {
    if cert.is_some() || key.is_some() {
        bail!(
            "--tls-cert and --tls-key need a build with the `tls` feature, and this one has none: rebuild with `cargo build --release -p doze-kafka --features tls` (it needs a C compiler)"
        );
    }
    Ok(())
}
