//! Server and signing certificates: PEM X.509 read for its expiry, a server certificate's key checked against it. The test data was
//! made with openssl for these tests (RSA 2048 and P-256, valid until 2126, and one valid for a day).

use serde_json::json;

use super::World;

const RSA_CERT: &str = include_str!("testdata/rsa-cert.pem");
const RSA_KEY: &str = include_str!("testdata/rsa-key.pem");
const RSA_PKCS1_KEY: &str = include_str!("testdata/rsa-pkcs1-key.pem");
const OTHER_CERT: &str = include_str!("testdata/other-cert.pem");
const OTHER_KEY: &str = include_str!("testdata/other-key.pem");
const EC_CERT: &str = include_str!("testdata/ec-cert.pem");
const EC_KEY: &str = include_str!("testdata/ec-key.pem");
/// Valid for one day from 2026-11-18.
const SHORT_CERT: &str = include_str!("testdata/short-cert.pem");

fn upload(
    w: &mut World,
    name: &str,
    body: &str,
    key: &str,
) -> Result<serde_json::Value, (String, String)> {
    w.run(
        "UploadServerCertificate",
        &json!({"ServerCertificateName": name, "CertificateBody": body, "PrivateKey": key, "Path": "/web/"}),
    )
    .map_err(|e| (e.shape.map_or_else(|| e.code.to_string(), |s| s.to_string()), e.message))
}

#[test]
fn a_server_certificate_is_read_for_its_expiry_and_its_key_must_be_its_own() {
    let mut w = World::new();
    let made = upload(&mut w, "site", RSA_CERT, RSA_KEY).unwrap();
    let meta = &made["ServerCertificateMetadata"];
    assert_eq!(
        meta["Arn"],
        "arn:aws:iam::000000000000:server-certificate/web/site"
    );
    assert!(
        meta["ServerCertificateId"]
            .as_str()
            .unwrap()
            .starts_with("ASCA")
    );
    // 2126-10-25T17:34:20Z, the certificate's notAfter (a GeneralizedTime).
    assert_eq!(meta["Expiration"].as_f64(), Some(4_948_623_260.0));
    assert!(upload(&mut w, "pkcs1", RSA_CERT, RSA_PKCS1_KEY).is_ok());
    assert!(upload(&mut w, "ec", EC_CERT, EC_KEY).is_ok());
    assert_eq!(
        upload(&mut w, "x", RSA_CERT, OTHER_KEY).unwrap_err().0,
        "KeyPairMismatchException"
    );
    assert_eq!(
        upload(&mut w, "y", EC_CERT, RSA_KEY).unwrap_err().0,
        "KeyPairMismatchException"
    );
    assert_eq!(
        upload(&mut w, "z", "not a cert", RSA_KEY).unwrap_err().0,
        "MalformedCertificateException"
    );
    assert_eq!(
        upload(&mut w, "z", RSA_CERT, "not a key").unwrap_err().0,
        "MalformedCertificateException"
    );
    assert_eq!(
        upload(&mut w, "site", OTHER_CERT, OTHER_KEY).unwrap_err().0,
        "EntityAlreadyExistsException"
    );
    // A one-day certificate, after its day.
    w.clock.set(1_795_200_000_000);
    assert_eq!(
        upload(&mut w, "late", SHORT_CERT, RSA_KEY).unwrap_err().0,
        "MalformedCertificateException"
    );
}

#[test]
fn a_server_certificate_is_returned_renamed_tagged_and_deleted() {
    let mut w = World::new();
    upload(&mut w, "site", RSA_CERT, RSA_KEY).unwrap();
    upload(&mut w, "pkcs1", RSA_CERT, RSA_PKCS1_KEY).unwrap();
    upload(&mut w, "ec", EC_CERT, EC_KEY).unwrap();
    let got = w.ok(
        "GetServerCertificate",
        &json!({"ServerCertificateName": "site"}),
    )["ServerCertificate"]
        .clone();
    assert_eq!(
        got["CertificateBody"],
        RSA_CERT.trim(),
        "without its closing newline"
    );
    assert!(got.get("PrivateKey").is_none());
    w.ok(
        "UpdateServerCertificate",
        &json!({"ServerCertificateName": "site", "NewServerCertificateName": "site2", "NewPath": "/"}),
    );
    let listed = w.ok("ListServerCertificates", &json!({"PathPrefix": "/web/"}))["ServerCertificateMetadataList"].clone();
    assert_eq!(listed.as_array().unwrap().len(), 2, "pkcs1 and ec");
    assert_eq!(
        w.ok(
            "GetServerCertificate",
            &json!({"ServerCertificateName": "site2"})
        )["ServerCertificate"]["ServerCertificateMetadata"]["Arn"],
        "arn:aws:iam::000000000000:server-certificate/site2"
    );
    w.ok(
        "TagServerCertificate",
        &json!({"ServerCertificateName": "site2", "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    assert_eq!(
        w.ok(
            "ListServerCertificateTags",
            &json!({"ServerCertificateName": "site2"})
        )["Tags"],
        json!([{"Key": "k", "Value": "v"}])
    );
    w.ok(
        "UntagServerCertificate",
        &json!({"ServerCertificateName": "site2", "TagKeys": ["k"]}),
    );
    assert_eq!(
        w.ok("GetAccountSummary", &json!({}))["SummaryMap"]["ServerCertificates"],
        3
    );
    w.ok(
        "DeleteServerCertificate",
        &json!({"ServerCertificateName": "site2"}),
    );
    assert_eq!(
        w.err(
            "DeleteServerCertificate",
            &json!({"ServerCertificateName": "site2"})
        ),
        (
            "NoSuchEntityException".into(),
            "The Server Certificate with name site2 cannot be found.".into()
        )
    );
}

#[test]
fn a_chain_must_be_certificates() {
    let mut w = World::new();
    let ok = w.run(
        "UploadServerCertificate",
        &json!({"ServerCertificateName": "c", "CertificateBody": RSA_CERT, "PrivateKey": RSA_KEY, "CertificateChain": format!("{OTHER_CERT}{EC_CERT}")}),
    );
    assert!(ok.is_ok());
    let bad = w.run(
        "UploadServerCertificate",
        &json!({"ServerCertificateName": "d", "CertificateBody": RSA_CERT, "PrivateKey": RSA_KEY, "CertificateChain": RSA_KEY}),
    );
    assert_eq!(
        bad.unwrap_err().shape.as_deref(),
        Some("MalformedCertificateException")
    );
}

#[test]
fn signing_certificates_are_two_per_user_and_unique_in_the_account() {
    let mut w = World::new();
    w.ok("CreateUser", &json!({"UserName": "u"}));
    w.ok("CreateUser", &json!({"UserName": "v"}));
    let made = w.ok(
        "UploadSigningCertificate",
        &json!({"UserName": "u", "CertificateBody": RSA_CERT}),
    )["Certificate"]
        .clone();
    assert_eq!(made["Status"], "Active");
    let id = made["CertificateId"].as_str().unwrap().to_owned();
    assert!(id.len() >= 24);
    assert_eq!(
        w.err(
            "UploadSigningCertificate",
            &json!({"UserName": "v", "CertificateBody": RSA_CERT})
        )
        .0,
        "DuplicateCertificateException"
    );
    w.ok(
        "UploadSigningCertificate",
        &json!({"UserName": "u", "CertificateBody": OTHER_CERT}),
    );
    assert_eq!(
        w.err(
            "UploadSigningCertificate",
            &json!({"UserName": "u", "CertificateBody": EC_CERT})
        )
        .0,
        "LimitExceededException"
    );
    w.ok(
        "UpdateSigningCertificate",
        &json!({"UserName": "u", "CertificateId": id, "Status": "Inactive"}),
    );
    let listed = w.ok("ListSigningCertificates", &json!({"UserName": "u"}))["Certificates"].clone();
    let status = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["CertificateId"] == id.as_str())
        .unwrap()["Status"]
        .clone();
    assert_eq!(status, "Inactive");
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(
        w.err("DeleteUser", &json!({"UserName": "u"})).1,
        "Cannot delete entity, must delete certificates first."
    );
    assert_eq!(
        w.err(
            "DeleteSigningCertificate",
            &json!({"UserName": "v", "CertificateId": id})
        )
        .0,
        "NoSuchEntityException",
        "another user's"
    );
    for c in listed.as_array().unwrap() {
        w.ok(
            "DeleteSigningCertificate",
            &json!({"UserName": "u", "CertificateId": c["CertificateId"]}),
        );
    }
    w.ok("DeleteUser", &json!({"UserName": "u"}));
}
