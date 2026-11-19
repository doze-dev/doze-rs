//! Certificates and keys as IAM is given them: PEM text. A certificate is walked by hand (`der`) to its validity and its
//! `subjectPublicKeyInfo`; a private key (PKCS #1 or PKCS #8 RSA, SEC 1 or PKCS #8 EC on P-256 or P-384) is decoded by the crates
//! KMS already links and its public half compared with the certificate's.

use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};

use crate::der::tlv;

/// The most PEM blocks read from one text (a chain of certificates is a handful).
const MAX_BLOCKS: usize = 64;

/// A certificate's parts this module reads, borrowed from its DER.
#[derive(Debug)]
pub(crate) struct Parts<'a> {
    pub(crate) spki: &'a [u8],
    /// Not before and not after, in unix seconds; `None` when the dates are in a form this does not read.
    pub(crate) validity: Option<(i64, i64)>,
}

/// A certificate is `SEQUENCE { tbsCertificate SEQUENCE { [0] version OPTIONAL, serialNumber, signature, issuer, validity, subject,
/// subjectPublicKeyInfo, ... }, ... }`.
pub(crate) fn parse_der(der: &[u8]) -> Option<Parts<'_>> {
    let (certificate, _) = tlv(der, 0x30)?;
    let (tbs, _) = tlv(certificate.contents, 0x30)?;
    let mut rest = tbs.contents;
    if rest.first() == Some(&0xa0) {
        rest = tlv(rest, 0xa0)?.1;
    }
    // serialNumber, signature, issuer.
    for tag in [0x02, 0x30, 0x30] {
        rest = tlv(rest, tag)?.1;
    }
    let (validity, after) = tlv(rest, 0x30)?;
    let validity = time(validity.contents).and_then(|(from, more)| Some((from, time(more)?.0)));
    let (_subject, after) = tlv(after, 0x30)?;
    let (spki, _) = tlv(after, 0x30)?;
    Some(Parts {
        spki: spki.whole,
        validity,
    })
}

/// An X.509 `Time`: `UTCTime` (`YYMMDDHHMMSSZ`, years 1950 to 2049) or `GeneralizedTime` (`YYYYMMDDHHMMSSZ`), as unix seconds.
fn time(bytes: &[u8]) -> Option<(i64, &[u8])> {
    let (tag, digits) = match bytes.first()? {
        0x17 => (0x17, 2),
        0x18 => (0x18, 4),
        _ => return None,
    };
    let (t, rest) = tlv(bytes, tag)?;
    let text = std::str::from_utf8(t.contents).ok()?;
    let year: i64 = text.get(..digits)?.parse().ok()?;
    let year = match digits {
        2 if year < 50 => 2000 + year,
        2 => 1900 + year,
        _ => year,
    };
    let body = text.get(digits..)?.strip_suffix('Z')?;
    let stamp = format!(
        "{year:04}-{}-{}T{}:{}:{}Z",
        body.get(0..2)?,
        body.get(2..4)?,
        body.get(4..6)?,
        body.get(6..8)?,
        body.get(8..10)?
    );
    Some((aws_core::time::parse_rfc3339(&stamp)?, rest))
}

/// Every PEM block of `text`: its label and its bytes. `None` when a block does not end, or its body is not base64.
pub(crate) fn blocks(text: &str) -> Option<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let Some(label) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|l| l.strip_suffix("-----"))
        else {
            continue;
        };
        if out.len() >= MAX_BLOCKS {
            return None;
        }
        let end = format!("-----END {label}-----");
        let mut body = String::new();
        loop {
            let line = lines.next()?;
            if line == end {
                break;
            }
            // Headers (`Proc-Type: ...`) mean an encrypted key, which is refused by its decoder.
            body.push_str(line);
        }
        out.push((label.to_owned(), aws_core::base64::decode(&body)?));
    }
    Some(out)
}

/// What a certificate's text holds: its single certificate's expiry and key.
#[derive(Debug)]
pub(crate) struct Cert {
    pub(crate) not_after_s: i64,
    spki: Vec<u8>,
}

/// The one certificate a body must be.
// fuzz:parser fuzz_certificate
pub(crate) fn certificate(text: &str) -> Option<Cert> {
    let blocks = blocks(text)?;
    let [(label, der)] = blocks.as_slice() else {
        return None;
    };
    if label != "CERTIFICATE" {
        return None;
    }
    let p = parse_der(der)?;
    let (_, not_after_s) = p.validity?;
    Some(Cert {
        not_after_s,
        // Owned: the PEM's bytes are gone when the call returns, and the key check comes after.
        spki: p.spki.to_vec(),
    })
}

/// Whether a chain is one or more certificates and nothing else.
pub(crate) fn chain(text: &str) -> bool {
    blocks(text).is_some_and(|b| {
        !b.is_empty()
            && b.iter()
                .all(|(label, der)| label == "CERTIFICATE" && parse_der(der).is_some())
    })
}

/// How a private key compares with a certificate.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyCheck {
    Matches,
    Mismatch,
    /// Not a private key this reads (no PEM key block, an encrypted key, another algorithm).
    Unreadable,
}

/// Whether `key` (PEM) is the private half of `cert`'s public key.
// fuzz:parser fuzz_certificate
pub(crate) fn key_matches(cert: &Cert, key: &str) -> KeyCheck {
    let Some(blocks) = blocks(key) else {
        return KeyCheck::Unreadable;
    };
    let Some((label, der)) = blocks.iter().find(|(l, _)| l.ends_with("PRIVATE KEY")) else {
        return KeyCheck::Unreadable;
    };
    let public = match label.as_str() {
        "RSA PRIVATE KEY" => rsa::PrivateKey::from_pkcs1_der(der)
            .ok()
            .map(|k| Public::Rsa(k.to_public_key())),
        "EC PRIVATE KEY" => p256::SecretKey::from_sec1_der(der)
            .ok()
            .map(|k| Public::P256(k.public_key()))
            .or_else(|| {
                p384::SecretKey::from_sec1_der(der)
                    .ok()
                    .map(|k| Public::P384(k.public_key()))
            }),
        "PRIVATE KEY" => pkcs8(der),
        _ => None,
    };
    match public {
        None => KeyCheck::Unreadable,
        Some(p) if p.is(&cert.spki) => KeyCheck::Matches,
        Some(_) => KeyCheck::Mismatch,
    }
}

fn pkcs8(der: &[u8]) -> Option<Public> {
    if let Ok(k) = rsa::PrivateKey::from_pkcs8_der(der) {
        return Some(Public::Rsa(k.to_public_key()));
    }
    if let Ok(k) = p256::SecretKey::from_pkcs8_der(der) {
        return Some(Public::P256(k.public_key()));
    }
    p384::SecretKey::from_pkcs8_der(der)
        .ok()
        .map(|k| Public::P384(k.public_key()))
}

/// The public half of a private key.
enum Public {
    Rsa(rsa::PublicKey),
    P256(p256::PublicKey),
    P384(p384::PublicKey),
}

impl Public {
    /// Whether the certificate's key info is this key.
    fn is(&self, spki: &[u8]) -> bool {
        match self {
            Self::Rsa(k) => rsa::PublicKey::from_public_key_der(spki).is_ok_and(|c| c == *k),
            Self::P256(k) => p256::PublicKey::from_public_key_der(spki).is_ok_and(|c| c == *k),
            Self::P384(k) => p384::PublicKey::from_public_key_der(spki).is_ok_and(|c| c == *k),
        }
    }
}
