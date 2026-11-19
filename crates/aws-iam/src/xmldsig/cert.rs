//! The RSA key in an X.509 certificate. The certificate is the trust anchor as it stands: its validity dates and chain are not
//! checked, as AWS uses the certificate in the provider's metadata and does not enforce its expiry (memory: from the IAM SAML
//! provider documentation, not recorded).
//!
//! The DER is walked by hand to the `subjectPublicKeyInfo` (`crate::x509`), which `rsa` decodes.

use rsa::pkcs8::DecodePublicKey;

/// The RSA public key of the DER certificate `der`; `None` for anything else (not a certificate, or a key of another kind).
pub(super) fn rsa_key(der: &[u8]) -> Option<rsa::PublicKey> {
    let cert = crate::x509::parse_der(der)?;
    rsa::PublicKey::from_public_key_der(cert.spki).ok()
}
