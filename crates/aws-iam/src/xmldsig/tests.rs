//! The fixtures in `testdata` were signed by xml-crypto (an independent XML-DSig implementation, run by bun), so the
//! canonicalization here is proven against someone else's: `response-signed.xml` (the Response signed, RSA-SHA256, PrefixLists
//! on both the reference and `SignedInfo`), `assertion-signed.xml` (the Assertion signed, RSA-SHA512), `both-signed.xml` (the
//! Assertion signed with RSA-SHA1, then the Response with RSA-SHA256 and `#default` in its PrefixList). `assertion-c14n.xml`
//! is xml-crypto's own exclusive canonical form of the assertion in `response-signed.xml`. `metadata.xml` names the signing
//! certificate (and another for encryption); `other-metadata.xml` swaps the two.
//!
//! `testdata/gen.ts` wrote them (its header says how to run it again: in a scratch directory, with xml-crypto added and two
//! throwaway keys and self-signed certificates made by openssl). It builds a Response around one Assertion, signs it with
//! `SignedXml` (exc-c14n, the enveloped signature placed after the Issuer), checks each signature with xml-crypto, and writes
//! the files above. The keys are not kept; a new run makes new fixtures, and the tests hold for any.

#![allow(clippy::unwrap_used)]

use super::*;

const RESPONSE_SIGNED: &str = include_str!("testdata/response-signed.xml");
const ASSERTION_SIGNED: &str = include_str!("testdata/assertion-signed.xml");
const BOTH_SIGNED: &str = include_str!("testdata/both-signed.xml");
const ASSERTION_C14N: &str = include_str!("testdata/assertion-c14n.xml");
const METADATA_XML: &str = include_str!("testdata/metadata.xml");
const OTHER_METADATA: &str = include_str!("testdata/other-metadata.xml");

fn certs() -> Vec<Vec<u8>> {
    signing_certs(METADATA_XML).unwrap().certs
}

#[test]
fn metadata_gives_its_entity_and_only_its_signing_certificate() {
    let m = signing_certs(METADATA_XML).unwrap();
    assert_eq!(m.entity_id, "https://idp.example.com/saml");
    assert_eq!(m.certs.len(), 1);
    assert!(cert::rsa_key(m.certs.first().unwrap()).is_some());
    let other = signing_certs(OTHER_METADATA).unwrap();
    assert_ne!(other.certs, m.certs);
    let wrapped = format!(
        r#"<md:EntitiesDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata">{}</md:EntitiesDescriptor>"#,
        METADATA_XML.trim_start_matches("<?xml version=\"1.0\"?>")
    );
    assert_eq!(signing_certs(&wrapped).unwrap(), m);
}

#[test]
fn metadata_without_a_signing_certificate_is_refused() {
    let no_use = METADATA_XML.replace(r#"use="signing""#, r#"use="encryption""#);
    assert!(matches!(
        signing_certs(&no_use),
        Err(Refusal::NoCertificate(_))
    ));
    assert!(matches!(signing_certs("<a/>"), Err(Refusal::Unreadable(_))));
}

#[test]
fn a_signed_response_gives_its_assertion_as_xml_crypto_canonicalizes_it() {
    let assertion = verify(RESPONSE_SIGNED, &certs()).unwrap();
    assert_eq!(assertion, ASSERTION_C14N);
}

#[test]
fn a_signed_assertion_gives_itself_without_its_signature() {
    let assertion = verify(ASSERTION_SIGNED, &certs()).unwrap();
    assert!(assertion.starts_with(
        r#"<saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="_a1""#
    ));
    assert!(!assertion.contains("Signature"));
    assert_eq!(assertion, ASSERTION_C14N);
}

#[test]
fn both_signatures_are_checked_when_both_are_there() {
    assert_eq!(verify(BOTH_SIGNED, &certs()).unwrap(), ASSERTION_C14N);
    // The assertion's own signature is checked too: break it and the whole is refused.
    let broken = BOTH_SIGNED.replacen("<ds:SignatureValue>", "<ds:SignatureValue>AAAA", 2);
    assert!(verify(&broken, &certs()).is_err());
}

#[test]
fn the_assertion_given_back_reads_as_saml() {
    let assertion = verify(RESPONSE_SIGNED, &certs()).unwrap();
    let read = crate::saml::read(&assertion).unwrap();
    assert_eq!(read.name_id, "alice&co <a>");
    assert_eq!(read.role_session_name.as_deref(), Some("alice@example.com"));
}

#[test]
fn a_bare_signed_assertion_is_read_too() {
    // The assertion cut out of its response, with the namespaces it inherited declared on it.
    let start = ASSERTION_SIGNED.find("<saml:Assertion ").unwrap();
    let end = ASSERTION_SIGNED.find("</saml:Assertion>").unwrap() + "</saml:Assertion>".len();
    let bare = ASSERTION_SIGNED.get(start..end).unwrap().replacen(
        "<saml:Assertion ",
        r#"<saml:Assertion xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" "#,
        1,
    );
    assert_eq!(verify(&bare, &certs()).unwrap(), ASSERTION_C14N);
}

#[test]
fn line_ends_written_as_crlf_still_verify() {
    let crlf = RESPONSE_SIGNED.replace('\n', "\r\n");
    assert_eq!(verify(&crlf, &certs()).unwrap(), ASSERTION_C14N);
}

#[test]
fn a_tampered_name_id_breaks_the_digest() {
    for signed in [RESPONSE_SIGNED, ASSERTION_SIGNED] {
        let tampered = signed.replace("alice&amp;co", "mallory&amp;co");
        assert_eq!(verify(&tampered, &certs()), Err(Refusal::DigestMismatch));
    }
}

#[test]
fn a_tampered_signature_value_or_another_certificate_is_refused() {
    let tampered = RESPONSE_SIGNED.replacen("<ds:SignatureValue>", "<ds:SignatureValue>AAAA", 1);
    assert_eq!(verify(&tampered, &certs()), Err(Refusal::BadSignature));
    let other = signing_certs(OTHER_METADATA).unwrap().certs;
    assert_eq!(verify(RESPONSE_SIGNED, &other), Err(Refusal::BadSignature));
    // Either certificate of several may have signed.
    let both = [other, certs()].concat();
    assert!(verify(RESPONSE_SIGNED, &both).is_ok());
    assert!(matches!(
        verify(RESPONSE_SIGNED, &[]),
        Err(Refusal::NoCertificate(_))
    ));
    assert!(matches!(
        verify(RESPONSE_SIGNED, &[b"junk".to_vec()]),
        Err(Refusal::NoCertificate(_))
    ));
}

/// `xml` with its first `ds:Signature` element taken out.
fn unsigned(xml: &str) -> String {
    let start = xml.find("<ds:Signature ").unwrap();
    let end = xml.find("</ds:Signature>").unwrap() + "</ds:Signature>".len();
    format!("{}{}", xml.get(..start).unwrap(), xml.get(end..).unwrap())
}

#[test]
fn an_unsigned_response_is_refused() {
    assert_eq!(
        verify(&unsigned(RESPONSE_SIGNED), &certs()),
        Err(Refusal::NotSigned)
    );
}

#[test]
fn a_second_assertion_beside_the_signed_one_is_wrapping() {
    let forged = r#"<saml:Assertion ID="_forged" Version="2.0"><saml:Issuer>x</saml:Issuer><saml:Subject><saml:NameID>root</saml:NameID></saml:Subject></saml:Assertion>"#;
    for signed in [RESPONSE_SIGNED, ASSERTION_SIGNED] {
        let wrapped = signed.replacen("<saml:Assertion ", &format!("{forged}<saml:Assertion "), 1);
        assert!(
            matches!(verify(&wrapped, &certs()), Err(Refusal::Wrapping(_))),
            "{wrapped}"
        );
    }
}

#[test]
fn a_signed_assertion_moved_aside_for_a_forged_one_is_wrapping() {
    // The signed assertion kept inside an Extensions element, a forged one with the same ID where the reader looks.
    let start = ASSERTION_SIGNED.find("<saml:Assertion ").unwrap();
    let end = ASSERTION_SIGNED.find("</saml:Assertion>").unwrap() + "</saml:Assertion>".len();
    let signed = ASSERTION_SIGNED.get(start..end).unwrap();
    let forged = signed.replace("alice&amp;co", "root");
    let moved = ASSERTION_SIGNED.replacen(
        signed,
        &format!("<samlp:Extensions>{signed}</samlp:Extensions>{forged}"),
        1,
    );
    assert!(matches!(
        verify(&moved, &certs()),
        Err(Refusal::Wrapping(_))
    ));
    // Or, the forged one unsigned and given another ID.
    let renamed = moved.replace(
        &forged,
        &unsigned(&forged.replace("ID=\"_a1\"", "ID=\"_a2\"")),
    );
    assert_eq!(verify(&renamed, &certs()), Err(Refusal::NotSigned));
}

#[test]
fn a_reference_to_another_element_than_the_signatures_parent_is_wrapping() {
    let elsewhere = ASSERTION_SIGNED.replace(r##"URI="#_a1""##, r##"URI="#_r1""##);
    assert!(matches!(
        verify(&elsewhere, &certs()),
        Err(Refusal::Wrapping(_))
    ));
    let whole = ASSERTION_SIGNED.replace(r##"URI="#_a1""##, r#"URI="""#);
    assert!(matches!(
        verify(&whole, &certs()),
        Err(Refusal::Wrapping(_))
    ));
}

#[test]
fn duplicate_ids_are_refused() {
    let twice = RESPONSE_SIGNED.replace(r#"ID="_a1""#, r#"ID="_r1""#);
    assert!(matches!(
        verify(&twice, &certs()),
        Err(Refusal::Wrapping(_))
    ));
}

#[test]
fn a_doctype_is_refused() {
    let doctype = RESPONSE_SIGNED.replacen(
        "<samlp:Response",
        "<!DOCTYPE r [<!ENTITY x \"y\">]><samlp:Response",
        1,
    );
    assert!(matches!(
        verify(&doctype, &certs()),
        Err(Refusal::Unreadable(_))
    ));
}

#[test]
fn an_unsupported_algorithm_is_refused_by_name() {
    let exc = r#"Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#""#;
    let cases = [
        (
            "xmldsig-more#rsa-sha256",
            "xmldsig-more#ecdsa-sha256",
            "ecdsa-sha256",
        ),
        ("xmlenc#sha256", "xmlenc#sha384", "sha384"),
        (
            exc,
            r#"Algorithm="http://www.w3.org/TR/2001/REC-xml-c14n-20010315""#,
            "REC-xml-c14n",
        ),
        (
            exc,
            r#"Algorithm="http://www.w3.org/2001/10/xml-exc-c14n#WithComments""#,
            "with comments",
        ),
        ("xmldsig#enveloped-signature", "xmldsig#base64", "#base64"),
    ];
    for (from, to, named) in cases {
        let changed = RESPONSE_SIGNED.replacen(from, to, 1);
        assert_ne!(changed, RESPONSE_SIGNED, "{from}");
        match verify(&changed, &certs()) {
            Err(Refusal::Unsupported(what)) => assert!(what.contains(named), "{what}"),
            other => panic!("{from}: {other:?}"),
        }
    }
    let encrypted = RESPONSE_SIGNED.replace("saml:Assertion", "saml:EncryptedAssertion");
    assert_eq!(
        verify(&encrypted, &certs()).unwrap_err().message(),
        "An encrypted assertion is not supported."
    );
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for r in [
        Refusal::Unreadable("it is not XML".to_owned()),
        Refusal::NotSigned,
        Refusal::Unsupported("the digest method x".to_owned()),
        Refusal::DigestMismatch,
        Refusal::BadSignature,
        Refusal::NoCertificate("no certificate".to_owned()),
        Refusal::Wrapping("two assertions".to_owned()),
    ] {
        let m = r.to_string();
        assert!(m.starts_with(char::is_uppercase) && m.ends_with('.'), "{m}");
    }
}
