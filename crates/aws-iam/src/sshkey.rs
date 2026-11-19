//! SSH public keys as `UploadSSHPublicKey` takes them: `ssh-rsa AAAA... comment` (RFC 4253 section 6.6) or PEM (an X.509
//! `SubjectPublicKeyInfo`, or a PKCS #1 `RSA PUBLIC KEY`). Read by hand into the modulus and exponent, so either form can be written
//! back as the other (`GetSSHPublicKey`'s `Encoding`). The model: an RSA key of at least 2048 bits.

use md5::{Digest, Md5};

use crate::der::tlv;

/// `rsaEncryption` (1.2.840.113549.1.1.1), with its tag and length.
const RSA_OID: [u8; 11] = [
    0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
];
/// The model's minimum.
pub(crate) const MIN_BITS: usize = 2048;

/// Why a key was refused.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeyFault {
    /// Neither form, or a key type other than RSA.
    Encoding,
    /// The right form, but not a key (or too short).
    Invalid,
}

/// An RSA public key: big-endian modulus and exponent, without leading zeros.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RsaKey {
    n: Vec<u8>,
    e: Vec<u8>,
}

/// Reads an uploaded key.
// fuzz:parser fuzz_ssh_key
pub(crate) fn read(body: &str) -> Result<RsaKey, KeyFault> {
    let body = body.trim();
    let key = if body.starts_with("-----BEGIN") {
        from_pem(body)?
    } else {
        from_ssh(body)?
    };
    if bits(&key.n) < MIN_BITS {
        return Err(KeyFault::Invalid);
    }
    Ok(key)
}

fn from_ssh(body: &str) -> Result<RsaKey, KeyFault> {
    let mut words = body.split_whitespace();
    let kind = words.next().ok_or(KeyFault::Encoding)?;
    if kind != "ssh-rsa" {
        return Err(KeyFault::Encoding);
    }
    let blob = words
        .next()
        .and_then(aws_core::base64::decode)
        .ok_or(KeyFault::Invalid)?;
    let (name, rest) = ssh_string(&blob).ok_or(KeyFault::Invalid)?;
    let (e, rest) = ssh_string(rest).ok_or(KeyFault::Invalid)?;
    let (n, rest) = ssh_string(rest).ok_or(KeyFault::Invalid)?;
    if name != b"ssh-rsa" || !rest.is_empty() {
        return Err(KeyFault::Invalid);
    }
    Ok(RsaKey {
        n: trim(n),
        e: trim(e),
    })
}

fn from_pem(body: &str) -> Result<RsaKey, KeyFault> {
    let blocks = crate::x509::blocks(body).ok_or(KeyFault::Invalid)?;
    let [(label, der)] = blocks.as_slice() else {
        return Err(KeyFault::Invalid);
    };
    match label.as_str() {
        "PUBLIC KEY" => spki(der).ok_or(KeyFault::Invalid),
        "RSA PUBLIC KEY" => pkcs1(der).ok_or(KeyFault::Invalid),
        _ => Err(KeyFault::Encoding),
    }
}

/// `SEQUENCE { SEQUENCE { rsaEncryption, NULL }, BIT STRING { RSAPublicKey } }`.
fn spki(der: &[u8]) -> Option<RsaKey> {
    let (outer, _) = tlv(der, 0x30)?;
    let (algorithm, rest) = tlv(outer.contents, 0x30)?;
    if !algorithm.contents.starts_with(&RSA_OID) {
        return None;
    }
    let (bits, _) = tlv(rest, 0x03)?;
    let (unused, key) = bits.contents.split_first()?;
    if *unused != 0 {
        return None;
    }
    pkcs1(key)
}

/// `RSAPublicKey ::= SEQUENCE { modulus INTEGER, publicExponent INTEGER }`.
fn pkcs1(der: &[u8]) -> Option<RsaKey> {
    let (seq, _) = tlv(der, 0x30)?;
    let (n, rest) = tlv(seq.contents, 0x02)?;
    let (e, _) = tlv(rest, 0x02)?;
    Some(RsaKey {
        n: trim(n.contents),
        e: trim(e.contents),
    })
}

/// An SSH `string`: a 32-bit length, then that many bytes.
fn ssh_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let len: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let len = usize::try_from(u32::from_be_bytes(len)).ok()?;
    let end = len.checked_add(4)?;
    Some((bytes.get(4..end)?, bytes.get(end..)?))
}

fn trim(int: &[u8]) -> Vec<u8> {
    let start = int.iter().take_while(|b| **b == 0).count();
    int.get(start..).unwrap_or_default().to_vec()
}

fn bits(n: &[u8]) -> usize {
    n.first().map_or(0, |top| {
        (n.len() - 1) * 8 + (8 - usize::try_from(top.leading_zeros()).unwrap_or(8))
    })
}

impl RsaKey {
    /// The SSH wire form (`string "ssh-rsa"`, `mpint e`, `mpint n`).
    fn blob(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for part in [b"ssh-rsa".as_slice(), &mpint(&self.e), &mpint(&self.n)] {
            out.extend(u32::try_from(part.len()).unwrap_or_default().to_be_bytes());
            out.extend(part);
        }
        out
    }

    /// The MD5 fingerprint of the SSH form, as `ssh-keygen -l -E md5` writes it.
    pub(crate) fn fingerprint(&self) -> String {
        Md5::digest(self.blob())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// `ssh-rsa <base64>`.
    pub(crate) fn ssh(&self) -> String {
        format!("ssh-rsa {}", aws_core::base64::encode(&self.blob()))
    }

    /// The key as a PEM `PUBLIC KEY` (an X.509 `SubjectPublicKeyInfo`).
    pub(crate) fn pem(&self) -> String {
        let rsa_key = der(
            0x30,
            &[der(0x02, &mpint(&self.n)), der(0x02, &mpint(&self.e))].concat(),
        );
        let algorithm = der(0x30, &[RSA_OID.as_slice(), &[0x05, 0x00]].concat());
        let bits = der(0x03, &[&[0_u8][..], &rsa_key].concat());
        let spki = der(0x30, &[algorithm, bits].concat());
        let text = aws_core::base64::encode(&spki);
        let lines: Vec<&str> = text
            .as_bytes()
            .chunks(64)
            .filter_map(|c| std::str::from_utf8(c).ok())
            .collect();
        format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            lines.join("\n")
        )
    }
}

/// A positive integer with the zero byte it needs when its top bit is set.
fn mpint(int: &[u8]) -> Vec<u8> {
    if int.first().is_some_and(|b| b & 0x80 != 0) {
        [&[0_u8][..], int].concat()
    } else {
        int.to_vec()
    }
}

/// A DER element.
fn der(tag: u8, contents: &[u8]) -> Vec<u8> {
    let len = contents.len();
    let mut out = vec![tag];
    if len < 0x80 {
        out.push(u8::try_from(len).unwrap_or_default());
    } else {
        let bytes: Vec<u8> = len
            .to_be_bytes()
            .into_iter()
            .skip_while(|b| *b == 0)
            .collect();
        out.push(0x80 | u8::try_from(bytes.len()).unwrap_or_default());
        out.extend(bytes);
    }
    out.extend(contents);
    out
}
