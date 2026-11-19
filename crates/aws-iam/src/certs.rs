//! Server certificates (the account's, for HTTPS listeners) and signing certificates (a user's, for SOAP-era request signing). Both
//! are PEM X.509 read by `x509`: a body that is not one certificate is malformed, its expiry is the certificate's own, and a server
//! certificate's private key must be the certificate's. Nothing serves TLS with them locally: they are kept, listed and returned.
//!
//! Wording marked (memory) was not seen recorded.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::entity::{arn_of, new_id};
use crate::errors::{exists, limit, no_such};
use crate::held::{CredKind, ServerCert, UserCred};
use crate::req::{Req, page, secs, tag_list};
use crate::x509::{self, KeyCheck};

/// `GetAccountSummary`'s quotas.
pub(crate) const SERVER_CERTS: usize = 20;
const SIGNING_PER_USER: usize = 2;
const MAX_TAGS: usize = 50;

fn malformed(message: &str) -> ApiError {
    ApiError::modeled("MalformedCertificateException", message)
}

/// The certificate a body holds, refused as AWS refuses a malformed or expired one (memory: the sentences).
fn read_certificate(body: &str, now_ms: i64) -> Result<x509::Cert, ApiError> {
    let cert = x509::certificate(body).ok_or_else(|| {
        malformed("Unable to parse certificate. Please ensure the certificate is in PEM format.")
    })?;
    if cert.not_after_s * 1000 <= now_ms {
        return Err(malformed("The certificate has expired."));
    }
    Ok(cert)
}

fn metadata(c: &ServerCert) -> Value {
    json!({
        "Path": c.path, "ServerCertificateName": c.name, "ServerCertificateId": c.id, "Arn": c.arn,
        "UploadDate": secs(c.uploaded_ms), "Expiration": secs(c.expires_ms),
    })
}

impl Core {
    fn server_cert(&self, name: &str) -> Result<&ServerCert, ApiError> {
        self.held
            .certs
            .get(name)
            .ok_or_else(|| no_such("Server Certificate", name))
    }

    pub(crate) fn upload_server_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ServerCertificateName");
        if self.held.certs.get(name).is_some() {
            return Err(exists("The Server Certificate", name));
        }
        if self.held.certs.len() >= SERVER_CERTS {
            return Err(limit(format!(
                "Cannot exceed quota for ServerCertificatesPerAccount: {SERVER_CERTS}"
            )));
        }
        let cert = read_certificate(r.s("CertificateBody"), self.now_ms())?;
        match x509::key_matches(&cert, r.s("PrivateKey")) {
            KeyCheck::Matches => {}
            // (memory)
            KeyCheck::Mismatch => {
                return Err(ApiError::modeled(
                    "KeyPairMismatchException",
                    "Public key certificate and private key doesn't match.",
                ));
            }
            KeyCheck::Unreadable => return Err(malformed("Unable to parse private key.")),
        }
        let chain = r.opt("CertificateChain");
        if chain.is_some_and(|c| !x509::chain(c)) {
            return Err(malformed("Unable to parse certificate chain."));
        }
        let path = r.opt("Path").unwrap_or("/");
        let c = ServerCert {
            name: name.to_owned(),
            path: path.to_owned(),
            id: new_id("ASCA")?,
            arn: arn_of(r.account(), "server-certificate", path, name),
            body: r.s("CertificateBody").to_owned(),
            chain: chain.map(str::to_owned),
            uploaded_ms: self.now_ms(),
            expires_ms: cert.not_after_s * 1000,
            tags: r.tags().into_iter().collect(),
        };
        let mut out = Map::new();
        out.insert("ServerCertificateMetadata".into(), metadata(&c));
        if !c.tags.is_empty() {
            out.insert("Tags".into(), tag_list(c.tags.iter()));
        }
        self.held.certs.insert(name.to_owned(), c);
        Ok(Value::Object(out))
    }

    pub(crate) fn get_server_certificate(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.server_cert(r.s("ServerCertificateName"))?;
        let mut m = Map::new();
        m.insert("ServerCertificateMetadata".into(), metadata(c));
        m.insert("CertificateBody".into(), json!(c.body));
        if let Some(chain) = &c.chain {
            m.insert("CertificateChain".into(), json!(chain));
        }
        m.insert("Tags".into(), tag_list(c.tags.iter()));
        Ok(json!({"ServerCertificate": m}))
    }

    pub(crate) fn list_server_certificates(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("PathPrefix").unwrap_or("/");
        let items = self
            .held
            .certs
            .iter()
            .map(|(_, c)| c)
            .filter(|c| c.path.starts_with(prefix))
            .map(metadata)
            .collect();
        page(r, items, "ServerCertificateMetadataList")
    }

    pub(crate) fn update_server_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let old = r.s("ServerCertificateName");
        let mut c = self.server_cert(old)?.clone();
        if let Some(new) = r.opt("NewServerCertificateName")
            && new != old
        {
            if self.held.certs.get(new).is_some() {
                return Err(exists("The Server Certificate", new));
            }
            new.clone_into(&mut c.name);
        }
        if let Some(path) = r.opt("NewPath") {
            path.clone_into(&mut c.path);
        }
        c.arn = arn_of(r.account(), "server-certificate", &c.path, &c.name);
        self.held.certs.remove(old);
        self.held.certs.insert(c.name.clone(), c);
        Ok(Value::Null)
    }

    pub(crate) fn delete_server_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ServerCertificateName");
        self.server_cert(name)?;
        self.held.certs.remove(name);
        Ok(Value::Null)
    }

    pub(crate) fn tag_server_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new: BTreeMap<String, String> = r.tags().into_iter().collect();
        let name = r.s("ServerCertificateName");
        let c = self
            .held
            .certs
            .get_mut(name)
            .ok_or_else(|| no_such("Server Certificate", name))?;
        let added = new.keys().filter(|k| !c.tags.contains_key(*k)).count();
        if c.tags.len() + added > MAX_TAGS {
            return Err(limit(
                "Cannot exceed quota for TagsPerServerCertificate: 50",
            ));
        }
        c.tags.extend(new);
        Ok(Value::Null)
    }

    pub(crate) fn untag_server_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ServerCertificateName");
        let keys = r.strings("TagKeys");
        let c = self
            .held
            .certs
            .get_mut(name)
            .ok_or_else(|| no_such("Server Certificate", name))?;
        for k in keys {
            c.tags.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_server_certificate_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.server_cert(r.s("ServerCertificateName"))?;
        let Value::Array(items) = tag_list(c.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }
}

/// Signing certificates.
impl Core {
    /// The IAM user a credential call is for: named, or the caller. The root's own credentials are not kept here.
    pub(crate) fn credential_user(&self, r: &Req<'_>) -> Result<String, ApiError> {
        self.named_or_caller(r)?.ok_or_else(|| {
            ApiError::modeled(
                "NoSuchEntityException",
                "doze-aws keeps these credentials for IAM users only, and the caller is not one: name the user (UserName).",
            )
        })
    }

    /// The credential `id` of `user`.
    pub(crate) fn user_cred_mut(
        &mut self,
        user: &str,
        id: &str,
        missing: fn(&str) -> ApiError,
    ) -> Result<&mut UserCred, ApiError> {
        match self.held.creds.get_mut(id) {
            Some(c) if c.user == user => Ok(c),
            _ => Err(missing(id)),
        }
    }

    pub(crate) fn upload_signing_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let body = r.s("CertificateBody");
        read_certificate(body, self.now_ms())?;
        let held = |c: &UserCred| matches!(&c.kind, CredKind::Signing { body: b } if b.trim() == body.trim());
        if self.held.creds.iter().any(|(_, c)| held(c)) {
            // (memory)
            return Err(ApiError::modeled(
                "DuplicateCertificateException",
                "The same certificate is already associated with an IAM user in the account.",
            ));
        }
        if self.signing_of(&user).count() >= SIGNING_PER_USER {
            return Err(limit(format!(
                "Cannot exceed quota for SigningCertificatesPerUser: {SIGNING_PER_USER}"
            )));
        }
        // (memory) AWS's certificate ids are upper-case letters and digits; 32 of them here.
        let id = aws_seal::alphanumeric(32)
            .ok_or_else(|| ApiError::internal("no randomness"))?
            .to_ascii_uppercase();
        let c = UserCred {
            id: id.clone(),
            user,
            active: true,
            created_ms: self.now_ms(),
            kind: CredKind::Signing {
                body: body.to_owned(),
            },
        };
        let shown = signing_json(&c);
        self.held.creds.insert(id, c);
        Ok(json!({"Certificate": shown}))
    }

    /// A user's signing certificates, oldest first.
    pub(crate) fn signing_of<'a>(
        &'a self,
        user: &'a str,
    ) -> impl Iterator<Item = &'a UserCred> + 'a {
        let mut certs: Vec<&UserCred> = self
            .held
            .creds
            .iter()
            .map(|(_, c)| c)
            .filter(move |c| c.user == user && matches!(c.kind, CredKind::Signing { .. }))
            .collect();
        certs.sort_by_key(|c| c.created_ms);
        certs.into_iter()
    }

    pub(crate) fn list_signing_certificates(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let items = self.signing_of(&user).map(signing_json).collect();
        page(r, items, "Certificates")
    }

    pub(crate) fn update_signing_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let active = r.s("Status") == "Active";
        self.user_cred_mut(&user, r.s("CertificateId"), no_certificate)?
            .active = active;
        Ok(Value::Null)
    }

    pub(crate) fn delete_signing_certificate(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let id = r.s("CertificateId");
        self.user_cred_mut(&user, id, no_certificate)?;
        self.held.creds.remove(id);
        Ok(Value::Null)
    }
}

fn signing_json(c: &UserCred) -> Value {
    let body = match &c.kind {
        CredKind::Signing { body } => body.as_str(),
        CredKind::Ssh { .. } | CredKind::Service(_) => "",
    };
    json!({
        "UserName": c.user, "CertificateId": c.id, "CertificateBody": body,
        "Status": if c.active { "Active" } else { "Inactive" }, "UploadDate": secs(c.created_ms),
    })
}

fn no_certificate(id: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("The Certificate with id {id} cannot be found."),
    )
}
