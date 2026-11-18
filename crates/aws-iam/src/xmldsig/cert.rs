//! The RSA key in an X.509 certificate. The certificate is the trust anchor as it stands: its validity dates and chain are not
//! checked, as AWS uses the certificate in the provider's metadata and does not enforce its expiry (memory: from the IAM SAML
//! provider documentation, not recorded).
//!
//! The DER is walked by hand to the `subjectPublicKeyInfo` (a certificate is `SEQUENCE { tbsCertificate SEQUENCE { [0] version
//! OPTIONAL, serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo, ... }, ... }`), which `rsa` decodes.

use rsa::pkcs8::DecodePublicKey;

/// The RSA public key of the DER certificate `der`; `None` for anything else (not a certificate, or a key of another kind).
pub(super) fn rsa_key(der: &[u8]) -> Option<rsa::PublicKey> {
    let (certificate, _) = tlv(der, 0x30)?;
    let (tbs, _) = tlv(certificate.contents, 0x30)?;
    let mut rest = tbs.contents;
    if rest.first() == Some(&0xa0) {
        rest = tlv(rest, 0xa0)?.1;
    }
    // serialNumber, signature, issuer, validity, subject.
    for tag in [0x02, 0x30, 0x30, 0x30, 0x30] {
        rest = tlv(rest, tag)?.1;
    }
    let (spki, _) = tlv(rest, 0x30)?;
    rsa::PublicKey::from_public_key_der(spki.whole).ok()
}

/// One DER element.
struct Tlv<'a> {
    /// The element with its tag and length.
    whole: &'a [u8],
    contents: &'a [u8],
}

/// The element `bytes` starts with, which must carry the tag `tag`, and what follows it.
fn tlv(bytes: &[u8], tag: u8) -> Option<(Tlv<'_>, &[u8])> {
    if *bytes.first()? != tag {
        return None;
    }
    let first = *bytes.get(1)?;
    let (len, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 {
            return None;
        }
        let len = bytes
            .get(2..2 + n)?
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
        (len, 2 + n)
    };
    let end = header.checked_add(len)?;
    Some((
        Tlv {
            whole: bytes.get(..end)?,
            contents: bytes.get(header..end)?,
        },
        bytes.get(end..)?,
    ))
}
