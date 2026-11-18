//! One enveloped signature checked: its single `Reference` must name the element the signature sits in, the digest of that
//! element (canonical, without the signature) must match, and `SignedInfo` (canonical) must carry a PKCS#1 v1.5 RSA signature
//! one of the provider's keys made. ECDSA and DSA signature methods are refused by name: identity providers sign SAML with RSA,
//! and the certificates read here hold RSA keys only.

use sha2::Digest as _;

use super::Refusal;
use super::c14n::{EXC_C14N, EXC_C14N_COMMENTS, canonicalize};
use super::tree::Element;

pub(super) const DSIG: &str = "http://www.w3.org/2000/09/xmldsig#";
const ENVELOPED: &str = "http://www.w3.org/2000/09/xmldsig#enveloped-signature";

/// A hash a digest or a signature method names.
#[derive(Debug, Clone, Copy)]
enum Hash {
    Sha1,
    Sha256,
    Sha512,
}

impl Hash {
    fn of_digest(uri: &str) -> Result<Self, Refusal> {
        match uri {
            "http://www.w3.org/2000/09/xmldsig#sha1" => Ok(Self::Sha1),
            "http://www.w3.org/2001/04/xmlenc#sha256" => Ok(Self::Sha256),
            "http://www.w3.org/2001/04/xmlenc#sha512" => Ok(Self::Sha512),
            other => Err(Refusal::Unsupported(format!("the digest method {other}"))),
        }
    }

    fn of_signature(uri: &str) -> Result<Self, Refusal> {
        match uri {
            "http://www.w3.org/2000/09/xmldsig#rsa-sha1" => Ok(Self::Sha1),
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256" => Ok(Self::Sha256),
            "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512" => Ok(Self::Sha512),
            other => Err(Refusal::Unsupported(format!(
                "the signature method {other}"
            ))),
        }
    }

    fn digest(self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => sha1::Sha1::digest(bytes).to_vec(),
            Self::Sha256 => sha2::Sha256::digest(bytes).to_vec(),
            Self::Sha512 => sha2::Sha512::digest(bytes).to_vec(),
        }
    }

    fn verifies(self, key: &rsa::PublicKey, signed: &[u8], signature: &[u8]) -> bool {
        match self {
            Self::Sha1 => key.pkcs1v15_verify::<sha1::Sha1>(signed, signature),
            Self::Sha256 => key.pkcs1v15_verify::<sha2::Sha256>(signed, signature),
            Self::Sha512 => key.pkcs1v15_verify::<sha2::Sha512>(signed, signature),
        }
        .is_ok()
    }
}

/// Checks `signature`, a child of the last element of `path` (the elements from the document element down to the signed one),
/// against `keys`.
pub(super) fn check(
    path: &[&Element],
    signature: &Element,
    keys: &[rsa::PublicKey],
) -> Result<(), Refusal> {
    let signed = path.last().ok_or(Refusal::NotSigned)?;
    let info = only(signature, "SignedInfo")?;
    let value = decode(&only(signature, "SignatureValue")?.text())?;
    let info_prefixes = c14n_method(only(info, "CanonicalizationMethod")?)?;
    let method = Hash::of_signature(algorithm(only(info, "SignatureMethod")?))?;
    let mut references = info.named(DSIG, "Reference");
    let (Some(reference), None) = (references.next(), references.next()) else {
        return Err(Refusal::Wrapping(
            "the signature must hold exactly one reference".to_owned(),
        ));
    };
    let id = signed.attr("ID").unwrap_or_default();
    if id.is_empty() || reference.attr("URI").and_then(|u| u.strip_prefix('#')) != Some(id) {
        return Err(Refusal::Wrapping(format!(
            "the signature's reference does not name the {} it is in",
            signed.local()
        )));
    }
    let prefixes = transforms(reference)?;
    let digest_method = Hash::of_digest(algorithm(only(reference, "DigestMethod")?))?;
    let expected = decode(&only(reference, "DigestValue")?.text())?;
    let canonical = canonicalize(path, Some(signature), &prefixes);
    if digest_method.digest(canonical.as_bytes()) != expected {
        return Err(Refusal::DigestMismatch);
    }
    let mut info_path = path.to_vec();
    info_path.extend([signature, info]);
    let canonical_info = canonicalize(&info_path, None, &info_prefixes);
    if keys
        .iter()
        .any(|k| method.verifies(k, canonical_info.as_bytes(), &value))
    {
        Ok(())
    } else {
        Err(Refusal::BadSignature)
    }
}

/// The one child `ds:<local>` of `e`.
fn only<'a>(e: &'a Element, local: &'static str) -> Result<&'a Element, Refusal> {
    let mut found = e.named(DSIG, local);
    match (found.next(), found.next()) {
        (Some(one), None) => Ok(one),
        _ => Err(Refusal::Unreadable(format!(
            "the signature must hold exactly one {local}"
        ))),
    }
}

fn algorithm(e: &Element) -> &str {
    e.attr("Algorithm").unwrap_or_default()
}

/// The PrefixList of a canonicalization method, which must be exclusive canonicalization without comments.
fn c14n_method(method: &Element) -> Result<Vec<String>, Refusal> {
    match algorithm(method) {
        EXC_C14N => Ok(prefix_list(method)),
        EXC_C14N_COMMENTS => Err(Refusal::Unsupported(
            "exclusive canonicalization with comments".to_owned(),
        )),
        other => Err(Refusal::Unsupported(format!(
            "the canonicalization method {other}"
        ))),
    }
}

fn prefix_list(method: &Element) -> Vec<String> {
    method
        .named(EXC_C14N, "InclusiveNamespaces")
        .filter_map(|e| e.attr("PrefixList"))
        .flat_map(str::split_ascii_whitespace)
        .map(str::to_owned)
        .collect()
}

/// The PrefixList of a reference whose transforms are the enveloped signature and exclusive canonicalization, each once.
fn transforms(reference: &Element) -> Result<Vec<String>, Refusal> {
    let (mut enveloped, mut prefixes) = (false, None);
    let list = reference.named(DSIG, "Transforms");
    for transform in list.flat_map(|t| t.named(DSIG, "Transform")) {
        match algorithm(transform) {
            ENVELOPED if !enveloped => enveloped = true,
            EXC_C14N | EXC_C14N_COMMENTS if prefixes.is_none() => {
                prefixes = Some(c14n_method(transform)?)
            }
            other => {
                return Err(Refusal::Unsupported(format!("the transform {other} here")));
            }
        }
    }
    match (enveloped, prefixes) {
        (true, Some(p)) => Ok(p),
        _ => Err(Refusal::Unsupported(
            "a reference without both the enveloped-signature and exclusive canonicalization transforms".to_owned(),
        )),
    }
}

/// Base64 with any whitespace in it.
fn decode(text: &str) -> Result<Vec<u8>, Refusal> {
    let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    aws_core::base64::decode(&compact)
        .ok_or_else(|| Refusal::Unreadable("a digest or signature value is not base64".to_owned()))
}
