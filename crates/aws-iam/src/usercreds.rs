//! A user's SSH public keys (for CodeCommit) and service-specific credentials (CodeCommit Git passwords, Keyspaces passwords, Bedrock
//! and CloudWatch Logs API keys). doze-aws runs none of the services that take them, so they are kept, listed and returned as AWS
//! does, and nothing accepts them. They stop a user from being deleted, as on AWS.
//!
//! Wording marked (memory) was not seen recorded.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::entity::new_id;
use crate::errors::limit;
use crate::held::{CredKind, Service, UserCred};
use crate::model::Kind;
use crate::req::{Req, page, secs};
use crate::sshkey::{self, KeyFault};

/// (memory) AWS's quota of SSH public keys per user.
const SSH_PER_USER: usize = 5;
/// The model: "a maximum of two sets of service-specific credentials for each supported service per user".
const PER_SERVICE: usize = 2;
const DAY_MS: i64 = 86_400_000;

/// The services that take service-specific credentials (memory: the IAM user guide), and whether theirs are API keys (an alias and a
/// secret, which may expire) rather than a user name and password.
const SERVICES: [(&str, bool); 4] = [
    ("codecommit.amazonaws.com", false),
    ("cassandra.amazonaws.com", false),
    ("bedrock.amazonaws.com", true),
    ("logs.amazonaws.com", true),
];

fn status(active: bool) -> &'static str {
    if active { "Active" } else { "Inactive" }
}

fn no_ssh_key(id: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("The Public Key with id {id} cannot be found."),
    )
}

fn no_credential(id: &str) -> ApiError {
    // Recorded 2026-11-19.
    ApiError::modeled(
        "NoSuchEntityException",
        format!("No such credential {id} exists"),
    )
}

fn random(n: usize) -> Result<String, ApiError> {
    aws_seal::alphanumeric(n).ok_or_else(|| ApiError::internal("no randomness"))
}

impl Core {
    fn ssh_keys_of<'a>(&'a self, user: &'a str) -> impl Iterator<Item = &'a UserCred> + 'a {
        self.held
            .creds
            .iter()
            .map(|(_, c)| c)
            .filter(move |c| c.user == user && matches!(c.kind, CredKind::Ssh { .. }))
    }

    pub(crate) fn upload_ssh_public_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName").to_owned();
        self.entity(Kind::User, &user)?;
        let body = r.s("SSHPublicKeyBody");
        let key = sshkey::read(body).map_err(|f| match f {
            // Recorded 2026-11-19.
            KeyFault::Encoding => ApiError::modeled(
                "UnrecognizedPublicKeyEncodingException",
                "Invalid public key uploaded.",
            ),
            KeyFault::Invalid => ApiError::modeled(
                "InvalidPublicKeyException",
                format!(
                    "The public key is not a valid RSA key of at least {} bits.",
                    sshkey::MIN_BITS
                ),
            ),
        })?;
        let fingerprint = key.fingerprint();
        let same = |c: &UserCred| matches!(&c.kind, CredKind::Ssh { fingerprint: f, .. } if *f == fingerprint);
        if self.ssh_keys_of(&user).any(same) {
            // Recorded 2026-11-19.
            return Err(ApiError::modeled(
                "DuplicateSSHPublicKeyException",
                "Duplicate SSH public key uploaded.",
            ));
        }
        if self.ssh_keys_of(&user).count() >= SSH_PER_USER {
            return Err(limit(format!(
                "Cannot exceed quota for PublicKeysPerUser: {SSH_PER_USER}"
            )));
        }
        let id = new_id("APKA")?;
        let c = UserCred {
            id: id.clone(),
            user,
            active: true,
            created_ms: self.now_ms(),
            kind: CredKind::Ssh {
                body: body.to_owned(),
                fingerprint,
            },
        };
        let shown = ssh_json(&c, None);
        self.held.creds.insert(id, c);
        Ok(json!({"SSHPublicKey": shown}))
    }

    pub(crate) fn get_ssh_public_key(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName");
        self.entity(Kind::User, user)?;
        let id = r.s("SSHPublicKeyId");
        let c = self
            .held
            .creds
            .get(id)
            .filter(|c| c.user == user)
            .ok_or_else(|| no_ssh_key(id))?;
        let CredKind::Ssh { body, .. } = &c.kind else {
            return Err(no_ssh_key(id));
        };
        let key = sshkey::read(body).map_err(|_| no_ssh_key(id))?;
        let shown = if r.s("Encoding") == "PEM" {
            key.pem()
        } else {
            key.ssh()
        };
        Ok(json!({"SSHPublicKey": ssh_json(c, Some(&shown))}))
    }

    pub(crate) fn list_ssh_public_keys(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let mut keys: Vec<&UserCred> = self.ssh_keys_of(&user).collect();
        keys.sort_by_key(|c| c.created_ms);
        let items = keys
            .into_iter()
            .map(|c| json!({"UserName": c.user, "SSHPublicKeyId": c.id, "Status": status(c.active), "UploadDate": secs(c.created_ms)}))
            .collect();
        page(r, items, "SSHPublicKeys")
    }

    pub(crate) fn update_ssh_public_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName");
        self.entity(Kind::User, user)?;
        let active = r.s("Status") == "Active";
        self.ssh_key_mut(user, r.s("SSHPublicKeyId"))?.active = active;
        Ok(Value::Null)
    }

    pub(crate) fn delete_ssh_public_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName");
        self.entity(Kind::User, user)?;
        let id = r.s("SSHPublicKeyId");
        self.ssh_key_mut(user, id)?;
        self.held.creds.remove(id);
        Ok(Value::Null)
    }

    fn ssh_key_mut(&mut self, user: &str, id: &str) -> Result<&mut UserCred, ApiError> {
        let c = self.user_cred_mut(user, id, no_ssh_key)?;
        match c.kind {
            CredKind::Ssh { .. } => Ok(c),
            CredKind::Signing { .. } | CredKind::Service(_) => Err(no_ssh_key(id)),
        }
    }
}

/// An SSH key as the calls show it; `body` is the key in the encoding asked for (an upload shows it as given).
fn ssh_json(c: &UserCred, body: Option<&str>) -> Value {
    let (given, fingerprint) = match &c.kind {
        CredKind::Ssh { body, fingerprint } => (body.as_str(), fingerprint.as_str()),
        CredKind::Signing { .. } | CredKind::Service(_) => ("", ""),
    };
    json!({
        "UserName": c.user, "SSHPublicKeyId": c.id, "Fingerprint": fingerprint, "SSHPublicKeyBody": body.unwrap_or(given),
        "Status": status(c.active), "UploadDate": secs(c.created_ms),
    })
}

/// Service-specific credentials.
impl Core {
    fn service_creds_of<'a>(
        &'a self,
        user: Option<&'a str>,
    ) -> impl Iterator<Item = (&'a UserCred, &'a Service)> + 'a {
        self.held
            .creds
            .iter()
            .filter_map(move |(_, c)| match &c.kind {
                CredKind::Service(s) if user.is_none_or(|u| c.user == u) => Some((c, s)),
                CredKind::Service(_) | CredKind::Signing { .. } | CredKind::Ssh { .. } => None,
            })
    }

    pub(crate) fn create_service_specific_credential(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let user = r.s("UserName").to_owned();
        self.entity(Kind::User, &user)?;
        let service = r.s("ServiceName");
        let api_key = api_key(service)?;
        if self
            .service_creds_of(Some(&user))
            .filter(|(_, s)| s.service == service)
            .count()
            >= PER_SERVICE
        {
            return Err(limit(format!(
                "Cannot exceed quota for ServiceSpecificCredentialsPerUserPerService: {PER_SERVICE}"
            )));
        }
        let now = self.now_ms();
        let held = Service {
            service: service.to_owned(),
            expires_ms: r
                .int("CredentialAgeDays")
                .filter(|_| api_key)
                .map(|d| now + d * DAY_MS),
            ..self.secrets(&user, r.account(), api_key)?
        };
        let id = new_id("ACCA")?;
        let c = UserCred {
            id: id.clone(),
            user,
            active: true,
            created_ms: now,
            kind: CredKind::Service(held),
        };
        let shown = service_json(&c, true);
        self.held.creds.insert(id, c);
        Ok(json!({"ServiceSpecificCredential": shown}))
    }

    /// New secrets: a user name and password, or an API key's alias and secret (memory: the shapes of AWS's generated values).
    fn secrets(&self, user: &str, account: &str, api_key: bool) -> Result<Service, ApiError> {
        let mut s = Service {
            service: String::new(),
            user_name: None,
            password: None,
            alias: None,
            secret: None,
            expires_ms: None,
        };
        if api_key {
            s.alias = Some(format!("{user}+{}-at-{account}", random(4)?));
            s.secret = Some(random(60)?);
        } else {
            // The model: "the IAM user's name combined with the ID number of the Amazon Web Services account".
            s.user_name = Some(format!("{user}-at-{account}"));
            s.password = Some(random(44)?);
        }
        Ok(s)
    }

    pub(crate) fn list_service_specific_credentials(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let all = r.flag("AllUsers").unwrap_or(false);
        if all && r.opt("UserName").is_some() {
            // The model: AllUsers "cannot be specified together with UserName". (memory: the words.)
            return Err(crate::errors::invalid_input(
                "AllUsers cannot be specified together with UserName.",
            ));
        }
        let user = if all {
            None
        } else {
            Some(self.credential_user(r)?)
        };
        if let Some(service) = r.opt("ServiceName") {
            api_key(service)?;
        }
        let items: Vec<Value> = self
            .service_creds_of(user.as_deref())
            .filter(|(_, s)| r.opt("ServiceName").is_none_or(|want| s.service == want))
            .map(|(c, _)| service_json(c, false))
            .collect();
        // Recorded 2026-11-19: the list says IsTruncated, as the other lists do.
        page(r, items, "ServiceSpecificCredentials")
    }

    pub(crate) fn reset_service_specific_credential(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let id = r.s("ServiceSpecificCredentialId");
        let api_key = match &self.user_cred_mut(&user, id, no_credential)?.kind {
            CredKind::Service(s) => api_key(&s.service)?,
            CredKind::Signing { .. } | CredKind::Ssh { .. } => return Err(no_credential(id)),
        };
        let fresh = self.secrets(&user, r.account(), api_key)?;
        let c = self.user_cred_mut(&user, id, no_credential)?;
        if let CredKind::Service(s) = &mut c.kind {
            // A reset gives a new password (or secret); the generated user name and alias stay.
            s.password = fresh.password.or(s.password.take());
            s.secret = fresh.secret.or(s.secret.take());
        }
        Ok(json!({"ServiceSpecificCredential": service_json(c, true)}))
    }

    pub(crate) fn update_service_specific_credential(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let active = r.s("Status") == "Active";
        let c = self.user_cred_mut(&user, r.s("ServiceSpecificCredentialId"), no_credential)?;
        c.active = active;
        Ok(Value::Null)
    }

    pub(crate) fn delete_service_specific_credential(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let user = self.credential_user(r)?;
        let id = r.s("ServiceSpecificCredentialId");
        if !matches!(
            self.user_cred_mut(&user, id, no_credential)?.kind,
            CredKind::Service(_)
        ) {
            return Err(no_credential(id));
        }
        self.held.creds.remove(id);
        Ok(Value::Null)
    }
}

/// Whether `service` takes API keys; refused when it takes no service-specific credentials.
fn api_key(service: &str) -> Result<bool, ApiError> {
    SERVICES
        .iter()
        .find(|(s, _)| *s == service)
        .map(|(_, api)| *api)
        .ok_or_else(|| {
            // Recorded 2026-11-19: AWS answers NoSuchEntity, not the model's ServiceNotSupported.
            ApiError::modeled(
                "NoSuchEntityException",
                format!("No such service {service} is supported for Service Specific Credentials"),
            )
        })
}

/// A credential as the calls show it; `secrets` is a create or a reset, the only times the password or secret is shown.
fn service_json(c: &UserCred, secrets: bool) -> Value {
    let mut m = Map::new();
    m.insert("UserName".into(), json!(c.user));
    m.insert("Status".into(), json!(status(c.active)));
    m.insert("ServiceSpecificCredentialId".into(), json!(c.id));
    m.insert("CreateDate".into(), secs(c.created_ms));
    if let CredKind::Service(s) = &c.kind {
        m.insert("ServiceName".into(), json!(s.service));
        let mut put = |k: &str, v: &Option<String>| {
            if let Some(v) = v {
                m.insert(k.into(), json!(v));
            }
        };
        put("ServiceUserName", &s.user_name);
        put("ServiceCredentialAlias", &s.alias);
        if secrets {
            put("ServicePassword", &s.password);
            put("ServiceCredentialSecret", &s.secret);
        }
        if let Some(at) = s.expires_ms {
            m.insert("ExpirationDate".into(), secs(at));
        }
    }
    Value::Object(m)
}
