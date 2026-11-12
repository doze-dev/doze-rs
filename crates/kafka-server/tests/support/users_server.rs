//! A broker with SASL and a super user, and the calls that make and delete users, for the tests of the credential APIs.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;

use bytes::Bytes;
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_auth::{Authenticator, Credentials, Mechanism, Users};
use kafka_broker::{Broker, Config as BrokerConfig};
use kafka_protocol::protocol::StrBytes;
use kafka_server::{Config, Running, Server};
use kafka_storage::sim::SimFs;
use kafka_wire::messages::alter_user_scram_credentials_request::{
    ScramCredentialDeletion, ScramCredentialUpsertion,
};
use kafka_wire::messages::{
    AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse, ApiKey,
};

use crate::client::Client;

pub(crate) const ADMIN_PW: &str = "admin-secret-planted-31";
pub(crate) const BAD: &str =
    "Authentication failed during authentication due to invalid credentials with SASL mechanism ";

/// A broker on `fs` whose only configured user is the super user `admin`. Starting it again on the same disk is a
/// restart: the stored users are loaded when it opens.
pub(crate) async fn start(fs: &SimFs) -> Running {
    let all = [
        Mechanism::Plain,
        Mechanism::ScramSha512,
        Mechanism::ScramSha256,
    ];
    let admin = [("admin".to_owned(), ADMIN_PW.to_owned())];
    let users = Arc::new(Users::derive(Credentials::new(admin).unwrap(), &all).unwrap());
    let auth = Authenticator::over(&all, Arc::clone(&users)).unwrap();
    let config = Config {
        listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        sasl: Some(Arc::new(auth)),
        ..Config::default()
    };
    let mut broker = BrokerConfig::new("/data".into());
    broker.acl_default_allow = false;
    broker.super_users.insert("User:admin".to_owned());
    broker.users = users;
    let broker = Broker::open(fs.clone(), broker).unwrap();
    Server::bind(config, Arc::new(broker)).await.unwrap()
}

pub(crate) async fn admin(server: &Running) -> Client {
    let mut c = Client::connect(server).await;
    c.scram_log_in(Kind::Sha256, "admin", ADMIN_PW).await;
    c
}

pub(crate) fn name(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

pub(crate) fn upsert(user: &str, kind: Kind, password: &str) -> ScramCredentialUpsertion {
    let (salt, iterations) = (format!("salt-{user}").into_bytes(), 4096);
    let mut u = ScramCredentialUpsertion::default();
    u.name = name(user);
    u.mechanism = if kind == Kind::Sha256 { 1 } else { 2 };
    u.iterations = iterations;
    let salted = kind.salted_password(
        password.as_bytes(),
        &salt,
        NonZeroU32::new(iterations.unsigned_abs()).unwrap(),
    );
    u.salted_password = Bytes::from(salted);
    u.salt = Bytes::from(salt);
    u
}

pub(crate) async fn alter(
    c: &mut Client,
    deletions: Vec<ScramCredentialDeletion>,
    upsertions: Vec<ScramCredentialUpsertion>,
) -> Vec<(String, i16)> {
    let mut req = AlterUserScramCredentialsRequest::default();
    req.deletions = deletions;
    req.upsertions = upsertions;
    let r: AlterUserScramCredentialsResponse = c
        .call(ApiKey::AlterUserScramCredentials, 0, &req)
        .await
        .unwrap();
    r.results
        .into_iter()
        .map(|x| (x.user.to_string(), x.error_code))
        .collect()
}

pub(crate) fn deletion(user: &str, mechanism: i8) -> ScramCredentialDeletion {
    let mut d = ScramCredentialDeletion::default();
    d.name = name(user);
    d.mechanism = mechanism;
    d
}

/// The login of `user` is refused: the answer is the generic failure and then the connection closes.
pub(crate) async fn refused(server: &Running, kind: Kind, user: &str, password: &str) {
    let mut c = Client::connect(server).await;
    let me = ScramClient::new(kind, user, password, "client-nonce-0001.");
    let mut said = c.scram_first(kind, &me).await;
    // An unknown user is turned down at once; a known user with the wrong password after the proof.
    if said.error_code == 0 {
        said = c
            .token(&me.last(&said.auth_bytes, None, "").message)
            .await
            .unwrap();
    }
    assert_eq!(said.error_code, 58, "{said:?}");
    let text = said.error_message.as_deref().unwrap_or_default();
    assert_eq!(text, format!("{BAD}{}", kind.mechanism().name()));
    assert!(c.is_closed().await);
}
