//! XML signatures on SAML, checked as AWS checks them for `AssumeRoleWithSAML`: the assertion's signature (or the response's)
//! must verify with a certificate from the IAM SAML provider's metadata document.
//!
//! The signature trusted is an enveloped one that is a direct child of the `Response` or of its `Assertion`, whose one reference
//! names that same element by its `ID`. Anything else is refused: that, with one assertion per response and unique IDs, is the
//! defence against signature wrapping (moving a signed element aside and putting a forged one where the reader looks). What the
//! caller reads its claims from is the canonical form of the signed assertion, given back by [`verify`]: bytes the signature
//! covered, and nothing beside them.

mod c14n;
mod cert;
mod reader;
mod signature;
mod tree;

use std::collections::HashSet;
use std::fmt;

use signature::DSIG;
use tree::{Element, Limits};

const PROTOCOL: &str = "urn:oasis:names:tc:SAML:2.0:protocol";
const ASSERTION: &str = "urn:oasis:names:tc:SAML:2.0:assertion";
const METADATA: &str = "urn:oasis:names:tc:SAML:2.0:metadata";

/// A response as `AssumeRoleWithSAML` takes it (the model allows 100 000 characters of base64, so less once decoded).
const RESPONSE_LIMITS: Limits = Limits {
    bytes: 100_000,
    depth: 32,
    elements: 10_000,
};

/// A metadata document as `CreateSAMLProvider` takes it (the model allows 10 000 000 characters).
const METADATA_LIMITS: Limits = Limits {
    bytes: 10_000_000,
    depth: 32,
    elements: 200_000,
};

/// Why a response or a metadata document is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Not well-formed, or not the document expected; the text says what was wrong.
    Unreadable(String),
    NotSigned,
    /// An algorithm, transform or form this check does not take; the text names it.
    Unsupported(String),
    DigestMismatch,
    BadSignature,
    /// No certificate the check can use; the text says why.
    NoCertificate(String),
    /// The signature does not cover what a reader would take as the assertion; the text says how.
    Wrapping(String),
}

impl Refusal {
    /// A short English sentence, for the caller to word AWS's error around.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unreadable(why) => format!("The document could not be read: {why}."),
            Self::NotSigned => "Neither the response nor its assertion is signed.".to_owned(),
            Self::Unsupported(what) => format!("{} is not supported.", capitalized(what)),
            Self::DigestMismatch => {
                "The signed element does not match the digest in its signature.".to_owned()
            }
            Self::BadSignature => {
                "The signature was not made by the provider's certificate.".to_owned()
            }
            Self::NoCertificate(why) => format!("{}.", capitalized(why)),
            Self::Wrapping(why) => format!("The signature does not cover the assertion: {why}."),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

fn capitalized(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// What an identity provider's metadata says about signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub(crate) entity_id: String,
    /// The DER of each signing certificate.
    pub(crate) certs: Vec<Vec<u8>>,
}

/// Reads an identity provider's metadata (an `EntityDescriptor`, or an `EntitiesDescriptor` holding one): its entity ID, and
/// every certificate of an `IDPSSODescriptor` key whose `use` is `signing` or not given.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "STS reads metadata through this once it checks signatures"
    )
)]
pub(crate) fn signing_certs(metadata: &str) -> Result<Metadata, Refusal> {
    let root = reader::parse(metadata, METADATA_LIMITS).map_err(Refusal::Unreadable)?;
    let entity = if root.is(METADATA, "EntitiesDescriptor") {
        let mut entities = root.named(METADATA, "EntityDescriptor");
        match (entities.next(), entities.next()) {
            (Some(one), None) => one,
            _ => {
                return Err(Refusal::Unsupported(
                    "metadata that does not describe exactly one entity".to_owned(),
                ));
            }
        }
    } else if root.is(METADATA, "EntityDescriptor") {
        &root
    } else {
        return Err(Refusal::Unreadable("it is not SAML metadata".to_owned()));
    };
    let entity_id = entity.attr("entityID").unwrap_or_default().to_owned();
    if entity_id.is_empty() {
        return Err(Refusal::Unreadable("the entity has no entityID".to_owned()));
    }
    let mut certs = Vec::new();
    let texts = entity
        .named(METADATA, "IDPSSODescriptor")
        .flat_map(|d| d.named(METADATA, "KeyDescriptor"))
        .filter(|k| k.attr("use").is_none_or(|u| u == "signing"))
        .flat_map(|k| k.named(DSIG, "KeyInfo"))
        .flat_map(|k| k.named(DSIG, "X509Data"))
        .flat_map(|d| d.named(DSIG, "X509Certificate"))
        .map(Element::text);
    for text in texts {
        let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        let der = aws_core::base64::decode(&compact)
            .ok_or_else(|| Refusal::Unreadable("a certificate is not base64".to_owned()))?;
        certs.push(der);
    }
    if certs.is_empty() {
        return Err(Refusal::NoCertificate(
            "the metadata names no signing certificate for an identity provider".to_owned(),
        ));
    }
    Ok(Metadata { entity_id, certs })
}

/// The signed assertion's XML, when `response` (decoded XML text: a `Response`, or a bare `Assertion`) carries a signature
/// one of `certs` (DER certificates) made. The XML given back is the exclusive canonical form of the assertion without its
/// signature: a standalone document, every namespace it uses declared, holding only what was signed.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "STS checks assertions through this once it checks signatures"
    )
)]
pub(crate) fn verify(response: &str, certs: &[Vec<u8>]) -> Result<String, Refusal> {
    let root = reader::parse(response, RESPONSE_LIMITS).map_err(Refusal::Unreadable)?;
    unique_ids(&root)?;
    let (outer, assertion) = if root.is(ASSERTION, "Assertion") {
        (None, &root)
    } else if root.is(PROTOCOL, "Response") {
        (Some(&root), the_assertion(&root)?)
    } else {
        return Err(Refusal::Unreadable(
            "it is neither a SAML Response nor an Assertion".to_owned(),
        ));
    };
    let outer_signature = outer.map(signature_of).transpose()?.flatten();
    let inner_signature = signature_of(assertion)?;
    if outer_signature.is_none() && inner_signature.is_none() {
        return Err(Refusal::NotSigned);
    }
    if certs.is_empty() {
        return Err(Refusal::NoCertificate(
            "the provider's metadata has no signing certificate".to_owned(),
        ));
    }
    let keys: Vec<_> = certs.iter().filter_map(|c| cert::rsa_key(c)).collect();
    if keys.is_empty() {
        return Err(Refusal::NoCertificate(
            "no certificate in the provider's metadata holds an RSA key".to_owned(),
        ));
    }
    let assertion_path: Vec<&Element> = outer.into_iter().chain([assertion]).collect();
    if let (Some(response), Some(sig)) = (outer, outer_signature) {
        signature::check(&[response], sig, &keys)?;
    }
    if let Some(sig) = inner_signature {
        signature::check(&assertion_path, sig, &keys)?;
    }
    Ok(c14n::canonicalize(&assertion_path, inner_signature, &[]))
}

/// The one assertion a response holds, as a direct child.
fn the_assertion(response: &Element) -> Result<&Element, Refusal> {
    if response
        .named(ASSERTION, "EncryptedAssertion")
        .next()
        .is_some()
    {
        return Err(Refusal::Unsupported("an encrypted assertion".to_owned()));
    }
    let mut assertions = response.named(ASSERTION, "Assertion");
    match (assertions.next(), assertions.next()) {
        (Some(one), None) => Ok(one),
        (None, _) => Err(Refusal::Unreadable(
            "the response holds no assertion".to_owned(),
        )),
        (Some(_), Some(_)) => Err(Refusal::Wrapping(
            "the response holds more than one assertion".to_owned(),
        )),
    }
}

/// The enveloped signature directly inside `e`, if any; two are refused.
fn signature_of(e: &Element) -> Result<Option<&Element>, Refusal> {
    let mut found = e.named(DSIG, "Signature");
    match (found.next(), found.next()) {
        (one, None) => Ok(one),
        (_, Some(_)) => Err(Refusal::Wrapping(format!(
            "the {} holds more than one signature",
            e.local()
        ))),
    }
}

/// Refuses a document in which two elements carry the same `ID` (or `Id`, or `id`): a reference must name exactly one.
fn unique_ids(root: &Element) -> Result<(), Refusal> {
    let mut seen = HashSet::new();
    let mut twin = None;
    root.walk(&mut |e| {
        for a in e
            .attrs
            .iter()
            .filter(|a| matches!(a.name.as_str(), "ID" | "Id" | "id"))
        {
            if !seen.insert(a.value.clone()) && twin.is_none() {
                twin = Some(a.value.clone());
            }
        }
    });
    match twin {
        Some(id) => Err(Refusal::Wrapping(format!("the ID {id} is used twice"))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
