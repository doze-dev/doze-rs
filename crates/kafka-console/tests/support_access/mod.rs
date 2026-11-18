//! Shared by the live tests of Settings and Access: a broker with SASL (SCRAM) on, `--acl-default deny`, a super user, a user from
//! flags with a planted password and a stored user with a planted salt, and the console's pages in front of it.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::sync::Arc;

use axum::Router;
use kafka_auth::scram::Kind;
use kafka_auth::{Authenticator, Credentials, Mechanism, Users};
use kafka_console::Backend;
use kafka_console::backend::requests::broker_configs;
use kafka_console::base::Base;
use kafka_console::facts::Facts;
use kafka_console::facts_start::StartFacts;
use kafka_console::source::{Live, Source};
use kafka_console::state::State;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::AlterUserScramCredentialsRequest;
use kafka_wire::messages::alter_user_scram_credentials_request::ScramCredentialUpsertion;
use tokio_util::sync::CancellationToken;

use crate::support_write::{World, post_form, world};

pub const FLAG_PASSWORD: &str = "PLANTED-FLAG-USER-PASSWORD-7731";
pub const STORED_SALT: &[u8] = b"PLANTED-SALT-0123";
pub const STORED_SALTED: &[u8] = b"PLANTED-SALTED-PASSWORD-32-BYTES";

/// A secured world and what was planted in it.
pub struct Secured {
    pub w: World,
    pub users: Arc<Users>,
}

/// Every form a secret could take on a page: the text, its hex and its base64.
pub fn forms(bytes: &[u8]) -> Vec<String> {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    vec![
        String::from_utf8_lossy(bytes).into_owned(),
        hex,
        base64(bytes),
    ]
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(T[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Everything planted that must never be shown: the password, the salts, the stored and server keys of every user.
pub fn secrets(s: &Secured) -> Vec<String> {
    let mut out = forms(FLAG_PASSWORD.as_bytes());
    out.extend(forms(STORED_SALT));
    out.extend(forms(STORED_SALTED));
    for kind in [Kind::Sha256, Kind::Sha512] {
        for user in ["admin", "payments"] {
            if let Some(v) = s.users.verifier(kind, user) {
                let (stored, server) = v.keys();
                for part in [v.salt(), stored, server] {
                    out.extend(forms(part));
                }
            }
        }
    }
    out
}

/// The secured world. `tls` only changes what the page is told (the listener in a test is plain TCP).
pub async fn secured(name: &str, tls: bool) -> Secured {
    let creds = Credentials::new([("admin".to_owned(), FLAG_PASSWORD.to_owned())]).unwrap();
    let mechanisms = [Mechanism::ScramSha256, Mechanism::ScramSha512];
    let users = Arc::new(Users::derive(creds, &mechanisms).unwrap());
    let auth = Authenticator::over(&mechanisms, Arc::clone(&users)).unwrap();
    let offered: Vec<String> = auth.offered().iter().map(|m| m.name().to_owned()).collect();
    let u = Arc::clone(&users);
    let mut w = world(name, move |b, s| {
        b.users = u;
        b.acl_default_allow = false;
        b.super_users.insert("User:admin".to_owned());
        s.sasl = Some(Arc::new(auth));
    })
    .await;
    w.facts.sasl = offered;
    w.facts.tls = tls;
    let s = Secured { w, users };
    store_user(&s, "payments", 1).await;
    s
}

/// Makes a stored user with the planted salt and salted password (as `kafka-configs.sh` would send them).
pub async fn store_user(s: &Secured, name: &str, mechanism: i8) {
    let mut u = ScramCredentialUpsertion::default();
    u.name = StrBytes::from_string(name.to_owned());
    u.mechanism = mechanism;
    u.iterations = 4096;
    u.salt = STORED_SALT.to_vec().into();
    u.salted_password = STORED_SALTED.to_vec().into();
    let mut r = AlterUserScramCredentialsRequest::default();
    r.upsertions = vec![u];
    let Response::AlterUserScramCredentials(r) =
        s.w.ask(0, Request::AlterUserScramCredentials(r)).await
    else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0, "{r:?}");
}

/// What the binary would tell the console of how it started.
pub fn start(tls: bool) -> StartFacts {
    StartFacts {
        flags: vec![],
        console_addr: Some("127.0.0.1:9093".to_owned()),
        tls_cert: tls.then(|| "/etc/doze/broker.pem".to_owned()),
        failed_login_delay: Some(std::time::Duration::from_millis(100)),
        console_unprotected: false,
    }
}

/// A router over `backend` with these facts.
pub fn router_with(facts: Facts, start: StartFacts, backend: Backend) -> Router {
    let live = Live::new(backend, facts).with_start(start);
    let state = State::new(
        Source::Live(Box::new(live)),
        Base::root(),
        vec![],
        CancellationToken::new(),
    );
    kafka_console::app::console(state)
}

impl Secured {
    pub fn router(&self) -> Router {
        router_with(
            self.w.facts.clone(),
            start(self.w.facts.tls),
            self.w.backend.clone(),
        )
    }
}

/// Every ACL the broker lists, as `(type, name, pattern, principal, host, operation, permission)`.
pub async fn acls(w: &World) -> Vec<(i8, String, i8, String, String, i8, i8)> {
    let Response::DescribeAcls(r) = w
        .ask(3, kafka_console::backend::requests::acls::describe_all())
        .await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    let mut out = Vec::new();
    for res in &r.resources {
        for a in &res.acls {
            out.push((
                res.resource_type,
                res.resource_name.to_string(),
                res.pattern_type,
                a.principal.to_string(),
                a.host.to_string(),
                a.operation,
                a.permission_type,
            ));
        }
    }
    out
}

pub fn new_acl(name: &str, principal: &str) -> Vec<(&'static str, String)> {
    vec![
        ("type", "topic".to_owned()),
        ("name", name.to_owned()),
        ("pattern", "literal".to_owned()),
        ("principal", principal.to_owned()),
        ("host", "*".to_owned()),
        ("op", "write".to_owned()),
        ("perm", "allow".to_owned()),
    ]
}

pub fn form(fields: &[(&'static str, String)]) -> axum::http::Request<axum::body::Body> {
    let f: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut req = post_form("/api/v1/access/acls/new", &f);
    req.headers_mut().insert("x-console", "1".parse().unwrap());
    req
}

/// A broker config as DescribeConfigs says it for this broker: value and source number.
pub async fn described(w: &World, key: &str) -> (String, i8) {
    let Response::DescribeConfigs(r) = w.ask(4, broker_configs::describe_broker(1)).await else {
        panic!()
    };
    let c = r.results[0]
        .configs
        .iter()
        .find(|c| c.name.as_str() == key)
        .unwrap_or_else(|| panic!("the broker does not list {key}"));
    (
        c.value
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        c.config_source,
    )
}

pub async fn alter(
    w: &World,
    name: &str,
    key: &str,
    value: &str,
    validate_only: bool,
) -> (i16, Option<String>) {
    let Response::IncrementalAlterConfigs(r) = w
        .ask(
            1,
            broker_configs::alter_broker(name, key, value, validate_only),
        )
        .await
    else {
        panic!()
    };
    (
        r.responses[0].error_code,
        r.responses[0]
            .error_message
            .as_ref()
            .map(ToString::to_string),
    )
}

pub async fn seeded(name: &str) -> World {
    let w = world(name, |b, _| {
        b.log_retention_check_every = std::time::Duration::from_secs(7);
        b.fsync_interval = std::time::Duration::from_millis(90);
    })
    .await;
    w.create_topic("orders", 2, &[("retention.ms", "604800000")])
        .await;
    w.create_topic("payments.settled", 1, &[]).await;
    w.create_topic("inventory.levels", 1, &[("cleanup.policy", "compact")])
        .await;
    w
}

pub fn app(w: &World) -> axum::Router {
    router_with(w.facts.clone(), start(false), w.backend.clone())
}
