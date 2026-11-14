//! TLS on the listener (cargo feature `tls`): a server certificate and key from PEM files, TLS 1.2 or 1.3, no client
//! certificates, and a handshake that runs inside the connection's task. The handshake completes before the first
//! Kafka byte is read; one that fails closes the connection and says only what kind of failure it was.
//!
//! A certificate or key that cannot be used is an error at start (the caller aborts), never a listener that quietly
//! speaks plaintext. The key is read once and never printed: not in an error, not in `Debug`, not in a log.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::crypto::ring::default_provider;
use rustls::pki_types::pem::{Error as PemError, PemObject};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::server::TlsStream;
use tokio_util::sync::CancellationToken;

pub(crate) use tokio_rustls::TlsAcceptor;

use crate::connection::End;

/// Why a certificate and key could not be loaded. None of these texts holds a byte of either file.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("cannot read {file}: {source}")]
    Read { file: String, source: io::Error },
    #[error("{file} holds no certificate (expected PEM, \"-----BEGIN CERTIFICATE-----\")")]
    NoCertificate { file: String },
    #[error(
        "{file} holds no private key (expected PEM: PKCS#8, SEC1 \"EC PRIVATE KEY\" or \"RSA PRIVATE KEY\")"
    )]
    NoKey { file: String },
    #[error(
        "the private key in {file} is encrypted; decrypt it first (openssl pkey -in KEY -out PLAIN)"
    )]
    EncryptedKey { file: String },
    #[error("{file} is not valid PEM")]
    BadPem { file: String },
    #[error("the private key in {key} does not match the certificate in {cert}")]
    Mismatch { cert: String, key: String },
    #[error("the private key in {file} is not one this build can use")]
    BadKey { file: String },
    #[error("setting up TLS failed: {0}")]
    Setup(rustls::Error),
}

/// A loaded certificate chain and key, ready to accept with. Cloning shares them.
#[derive(Clone)]
pub struct TlsConfig {
    cert: PathBuf,
    key: PathBuf,
    config: Arc<ServerConfig>,
}

impl TlsConfig {
    /// Reads and checks the certificate chain (leaf first) and the private key. TLS 1.2 is the oldest version
    /// offered; ring's own cipher suites; the server proves itself and asks nothing of the client.
    ///
    /// # Errors
    /// A file cannot be read, holds no certificate or key, or the two do not belong together.
    pub fn load(cert: impl Into<PathBuf>, key: impl Into<PathBuf>) -> Result<Self, TlsError> {
        let (cert, key) = (cert.into(), key.into());
        let (cert_pem, key_pem) = (read(&cert)?, read(&key)?);
        let config = build(&cert_pem, &key_pem, (&cert, &key))?;
        Ok(Self { cert, key, config })
    }

    /// The certificate file this was loaded from.
    #[must_use]
    pub fn cert_file(&self) -> &Path {
        &self.cert
    }

    pub(crate) fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(Arc::clone(&self.config))
    }

    /// The server configuration the certificate and key were loaded into: the schema registry serves its HTTPS with the
    /// broker's certificate (`--schema-registry-tls`).
    #[must_use]
    pub fn server_config(&self) -> Arc<rustls::ServerConfig> {
        Arc::clone(&self.config)
    }
}

impl fmt::Debug for TlsConfig {
    /// The files, not what is in them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
            .field("cert", &self.cert)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

fn read(file: &Path) -> Result<Vec<u8>, TlsError> {
    std::fs::read(file).map_err(|source| TlsError::Read {
        file: file.display().to_string(),
        source,
    })
}

fn build(
    cert_pem: &[u8],
    key_pem: &[u8],
    files: (&Path, &Path),
) -> Result<Arc<ServerConfig>, TlsError> {
    let (cert_file, key_file) = (files.0.display().to_string(), files.1.display().to_string());
    // The parser's own error would repeat the offending line of the file, which in a key file is key material.
    let chain = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, PemError>>()
        .map_err(|_| TlsError::BadPem {
            file: cert_file.clone(),
        })?;
    if chain.is_empty() {
        return Err(TlsError::NoCertificate { file: cert_file });
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|e| match e {
        PemError::NoItemsFound if key_pem.windows(9).any(|w| w == b"ENCRYPTED") => {
            TlsError::EncryptedKey {
                file: key_file.clone(),
            }
        }
        PemError::NoItemsFound => TlsError::NoKey {
            file: key_file.clone(),
        },
        _ => TlsError::BadPem {
            file: key_file.clone(),
        },
    })?;
    let provider = Arc::new(default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(TlsError::Setup)?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| match e {
            rustls::Error::InconsistentKeys(_) => TlsError::Mismatch {
                cert: cert_file,
                key: key_file,
            },
            _ => TlsError::BadKey { file: key_file },
        })?;
    Ok(Arc::new(config))
}

/// Runs the handshake on `socket` until `by` (the connection's pre-auth budget, counted from the accept) or until
/// `stop`. A failure is an [`End`] that names its kind and nothing the client sent.
pub(crate) async fn accept(
    acceptor: &TlsAcceptor,
    socket: TcpStream,
    by: Instant,
    stop: &CancellationToken,
) -> Result<TlsStream<TcpStream>, End> {
    tokio::select! {
        done = timeout_at(by, acceptor.accept(socket)) => match done {
            Err(_) => Err(End::HandshakeTooSlow),
            Ok(Err(e)) => Err(End::Handshake(kind(&e))),
            Ok(Ok(stream)) => Ok(stream),
        },
        () = stop.cancelled() => Err(End::Stopped),
    }
}

/// What went wrong, in words of ours. rustls' own texts can quote what the peer sent.
fn kind(e: &io::Error) -> &'static str {
    match e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        Some(rustls::Error::InvalidMessage(_)) => "not a TLS handshake",
        Some(rustls::Error::AlertReceived(_)) => "the client sent an alert",
        Some(rustls::Error::PeerIncompatible(_)) => "no protocol version or cipher suite in common",
        Some(rustls::Error::PeerMisbehaved(_)) => "a protocol violation",
        Some(_) => "a TLS error",
        None if e.kind() == io::ErrorKind::UnexpectedEof => "closed during the handshake",
        None => "an i/o error",
    }
}

#[cfg(test)]
mod tests;
