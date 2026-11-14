//! Outbound TLS: whom the client trusts, and what it says when a server's certificate is refused.
//!
//! **Trust** is the operating system's store (`os_roots.rs`; read once, on the first `https://` call, off the runtime's thread:
//! the macOS system-roots, system and login keychains, listed by `/usr/bin/security`, their trust settings not read; the Windows
//! certificate store; the Linux bundle `openssl-probe` finds or `SSL_CERT_FILE`), so a corporate proxy's CA, which is installed
//! there, is trusted. `--ca-file` adds roots (a local CA,
//! mkcert's, a self-signed server's own certificate); `--insecure-outbound-tls` verifies nothing (local development only, said
//! loudly at start). The OS store is read rather than handed to the platform's verifier (`rustls-platform-verifier`) because that
//! verifier may fetch revocation data inside the handshake, which would stall doze-aws's one thread.
//!
//! API Gateway's `tlsConfig.insecureSkipVerification` is the lenient mode: a certificate no trusted authority signed is accepted,
//! and its name is still checked (AWS also checks the expiry date; that is not checked here, see the stage note).

use std::path::Path;
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use tokio_rustls::TlsConnector;

/// Whom the client trusts.
#[derive(Clone, Debug, Default)]
pub struct Trust {
    /// Read the operating system's trust store (doze-aws always does; tests leave it out to be independent of the machine).
    pub os_store: bool,
    /// Roots added to it (`--ca-file`).
    pub extra: Vec<CertificateDer<'static>>,
    /// Verify nothing (`--insecure-outbound-tls`).
    pub insecure: bool,
}

impl Trust {
    /// What doze-aws runs with: the OS store, the certificates of `ca_file`, and the insecure switch.
    ///
    /// # Errors
    /// The file cannot be read, or holds no PEM certificate.
    pub fn new(ca_file: Option<&Path>, insecure: bool) -> Result<Self, String> {
        let extra = match ca_file {
            Some(path) => read_pem(path)?,
            None => Vec::new(),
        };
        Ok(Self {
            os_store: true,
            extra,
            insecure,
        })
    }
}

/// The certificates of a PEM file.
fn read_pem(path: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let shown = path.display();
    let bytes = std::fs::read(path).map_err(|e| format!("--ca-file {shown}: {e}"))?;
    let certs: Vec<_> = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("--ca-file {shown}: not PEM: {e}"))?;
    if certs.is_empty() {
        return Err(format!(
            "--ca-file {shown}: no certificate in it (a PEM file of one or more `-----BEGIN CERTIFICATE-----` blocks is expected)"
        ));
    }
    Ok(certs)
}

/// The two connectors: one that verifies, one for API Gateway's `insecureSkipVerification`.
#[derive(Clone)]
pub(crate) struct Connectors {
    pub(crate) strict: TlsConnector,
    pub(crate) lenient: TlsConnector,
}

impl std::fmt::Debug for Connectors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Connectors")
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Builds the connectors (blocking: the OS store is read here).
pub(crate) fn connectors(trust: &Trust) -> Result<Connectors, String> {
    let provider = provider();
    let algs = provider.signature_verification_algorithms;
    let config = |v: Arc<dyn ServerCertVerifier>| -> Result<TlsConnector, String> {
        let c = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(v)
            .with_no_client_auth();
        Ok(TlsConnector::from(Arc::new(c)))
    };
    if trust.insecure {
        let v: Arc<dyn ServerCertVerifier> = Arc::new(Anything(algs));
        return Ok(Connectors {
            strict: config(v.clone())?,
            lenient: config(v)?,
        });
    }
    let strict: Arc<dyn ServerCertVerifier> =
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots(trust)?), provider.clone())
            .build()
            .map_err(|e| e.to_string())?;
    let lenient = Arc::new(Lenient(strict.clone()));
    Ok(Connectors {
        strict: config(strict)?,
        lenient: config(lenient)?,
    })
}

fn roots(trust: &Trust) -> Result<RootCertStore, String> {
    let mut roots = RootCertStore::empty();
    let mut from_os = 0;
    if trust.os_store {
        let (certs, errors) = crate::os_roots::load();
        for e in &errors {
            tracing::debug!("outbound TLS: part of the OS trust store could not be read: {e}");
        }
        (from_os, _) = roots.add_parsable_certificates(certs);
        if from_os == 0 && trust.extra.is_empty() {
            return Err(format!(
                "the OS trust store could not be read ({}); start doze-aws with --ca-file <PEM> to name the authorities to trust",
                errors.join("; ")
            ));
        }
    }
    let (extra, refused) = roots.add_parsable_certificates(trust.extra.iter().cloned());
    if refused > 0 {
        tracing::warn!(
            "outbound TLS: {refused} certificate(s) of --ca-file are not usable as roots"
        );
    }
    tracing::info!("outbound TLS: {from_os} roots from the OS trust store, {extra} from --ca-file");
    if roots.is_empty() {
        return Err("there is no trusted root (the OS trust store is empty or unreadable, and no --ca-file is given)".to_owned());
    }
    Ok(roots)
}

/// What a refused handshake means, and the flag that fixes it.
pub(crate) fn explain(host: &str, port: u16, e: &std::io::Error) -> String {
    let at = format!("{host}:{port}");
    let tls = e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>());
    let Some(rustls::Error::InvalidCertificate(cert)) = tls else {
        return format!("the TLS handshake with {at} failed: {e}");
    };
    let flags = "start doze-aws with --ca-file <the CA certificate, PEM> to trust the authority that signed it, or with --insecure-outbound-tls to skip verification of outbound https calls (local development only)";
    match cert {
        CertificateError::UnknownIssuer => format!(
            "the certificate {at} presented is not signed by an authority this machine trusts (self-signed, or from a private CA). If it is your server, {flags}"
        ),
        CertificateError::Expired | CertificateError::ExpiredContext { .. } => format!(
            "the certificate {at} presented has expired: renew it, or start doze-aws with --insecure-outbound-tls"
        ),
        CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => format!(
            "the certificate {at} presented is not valid yet (is the clock right?); or start doze-aws with --insecure-outbound-tls"
        ),
        CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
            format!(
                "the certificate {at} presented is not for the name {host}: call the server by a name its certificate lists, or start doze-aws with --insecure-outbound-tls"
            )
        }
        other => format!("the certificate {at} presented was refused ({other:?}); {flags}"),
    }
}

/// The server name a handshake verifies.
pub(crate) fn server_name(host: &str) -> Result<ServerName<'static>, String> {
    ServerName::try_from(host.to_owned()).map_err(|_| format!("{host} is not a valid server name"))
}

/// Accepts any certificate (`--insecure-outbound-tls`); the handshake's signatures are still checked.
#[derive(Debug)]
struct Anything(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for Anything {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// Verifies, but accepts a certificate no trusted authority signed if it is for the name called (`insecureSkipVerification`).
#[derive(Debug)]
struct Lenient(Arc<dyn ServerCertVerifier>);

impl ServerCertVerifier for Lenient {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, name, ocsp, now)
        {
            Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {
                let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
                rustls::client::verify_server_name(&parsed, name)?;
                Ok(ServerCertVerified::assertion())
            }
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ca_file_is_read_or_refused_by_name() {
        let dir = std::env::temp_dir().join(format!("aws-webhook-ca-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty.pem");
        std::fs::write(&empty, "nothing here\n").unwrap();
        let e = Trust::new(Some(&empty), false).unwrap_err();
        assert!(e.contains("no certificate in it"), "{e}");
        let e = Trust::new(Some(&dir.join("missing.pem")), false).unwrap_err();
        assert!(e.starts_with("--ca-file "), "{e}");
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        let good = dir.join("good.pem");
        std::fs::write(&good, cert.pem()).unwrap();
        let t = Trust::new(Some(&good), true).unwrap();
        assert_eq!((t.extra.len(), t.insecure, t.os_store), (1, true, true));
        std::fs::remove_dir_all(&dir).ok();
    }
}
