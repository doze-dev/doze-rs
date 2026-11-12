//! Users made over the API, on a real socket: an admin makes one, it logs in at once and after a restart, a deleted one
//! cannot log in again while a connection that was already open stays logged in, and a user who is not a super user
//! can neither make nor list users.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "support/sasl_client.rs"]
mod client;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use client::{Client, metadata_for};
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
    DescribeUserScramCredentialsRequest, DescribeUserScramCredentialsResponse, MetadataResponse,
};
use std::num::NonZeroU32;

const ADMIN_PW: &str = "admin-secret-planted-31";
const BAD: &str =
    "Authentication failed during authentication due to invalid credentials with SASL mechanism ";

/// A broker on `fs` whose only configured user is the super user `admin`. Starting it again on the same disk is a
/// restart: the stored users are loaded when it opens.
async fn start(fs: &SimFs) -> Running {
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

async fn admin(server: &Running) -> Client {
    let mut c = Client::connect(server).await;
    c.scram_log_in(Kind::Sha256, "admin", ADMIN_PW).await;
    c
}

fn name(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

fn upsert(user: &str, kind: Kind, password: &str) -> ScramCredentialUpsertion {
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

async fn alter(
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

fn deletion(user: &str, mechanism: i8) -> ScramCredentialDeletion {
    let mut d = ScramCredentialDeletion::default();
    d.name = name(user);
    d.mechanism = mechanism;
    d
}

/// The login of `user` is refused: the answer is the generic failure and then the connection closes.
async fn refused(server: &Running, kind: Kind, user: &str, password: &str) {
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

#[tokio::test]
async fn a_user_made_by_an_admin_logs_in_at_once_and_after_a_restart_and_a_deleted_one_does_not() {
    let fs = SimFs::new();
    let server = start(&fs).await;
    let mut root = admin(&server).await;
    refused(&server, Kind::Sha512, "carol", "carol-pw").await;
    let made = alter(
        &mut root,
        vec![],
        vec![upsert("carol", Kind::Sha512, "carol-pw")],
    )
    .await;
    assert_eq!(made, vec![("carol".to_owned(), 0)]);
    let made = alter(
        &mut root,
        vec![],
        vec![upsert("carol", Kind::Sha256, "carol-pw")],
    )
    .await;
    assert_eq!(made, vec![("carol".to_owned(), 0)]);

    // At once, with either hash; a wrong password and PLAIN (which has no password to compare) are refused.
    let mut carol = Client::connect(&server).await;
    carol.scram_log_in(Kind::Sha512, "carol", "carol-pw").await;
    Client::connect(&server)
        .await
        .scram_log_in(Kind::Sha256, "carol", "carol-pw")
        .await;
    let mut wrong = Client::connect(&server).await;
    let me = ScramClient::new(Kind::Sha512, "carol", "nope", "client-nonce-0001.");
    let first = wrong.scram_first(Kind::Sha512, &me).await;
    let last = wrong
        .token(&me.last(&first.auth_bytes, None, "").message)
        .await
        .unwrap();
    assert_eq!(last.error_code, 58);
    let mut plain = Client::connect(&server).await;
    assert_eq!(plain.handshake("PLAIN").await.unwrap().error_code, 0);
    let said = plain.token(b"\0carol\0carol-pw").await.unwrap();
    assert_eq!(said.error_code, 58, "{said:?}");

    // She is not a super user: she may neither make nor list users, and says so in the shape Kafka answers.
    let denied = alter(&mut carol, vec![], vec![upsert("eve", Kind::Sha256, "x")]).await;
    assert_eq!(denied, vec![("eve".to_owned(), 31)]);
    let listed: DescribeUserScramCredentialsResponse = carol
        .call(
            ApiKey::DescribeUserScramCredentials,
            0,
            &DescribeUserScramCredentialsRequest::default(),
        )
        .await
        .unwrap();
    assert_eq!(listed.error_code, 31);

    // After a restart the stored user is still there and logs in.
    server.shutdown().await;
    let server = start(&fs.process_restart()).await;
    Client::connect(&server)
        .await
        .scram_log_in(Kind::Sha512, "carol", "carol-pw")
        .await;
    refused(&server, Kind::Sha512, "carol", "other").await;

    // Deleted: new logins with that mechanism are refused, the other mechanism still works, and the connection that
    // was open stays open (Kafka re-checks only at re-authentication).
    let mut root = admin(&server).await;
    let mut open = Client::connect(&server).await;
    open.scram_log_in(Kind::Sha256, "carol", "carol-pw").await;
    let gone = alter(&mut root, vec![deletion("carol", 2)], vec![]).await;
    assert_eq!(gone, vec![("carol".to_owned(), 0)]);
    refused(&server, Kind::Sha512, "carol", "carol-pw").await;
    Client::connect(&server)
        .await
        .scram_log_in(Kind::Sha256, "carol", "carol-pw")
        .await;
    let gone = alter(&mut root, vec![deletion("carol", 1)], vec![]).await;
    assert_eq!(gone, vec![("carol".to_owned(), 0)]);
    refused(&server, Kind::Sha256, "carol", "carol-pw").await;
    let still: MetadataResponse = open
        .call(ApiKey::Metadata, 9, &metadata_for("anything"))
        .await
        .unwrap();
    assert_eq!(
        still.topics.len(),
        1,
        "the connection that was open still works"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn the_user_configured_at_start_cannot_be_taken_over_and_is_listed() {
    let fs = SimFs::new();
    let server = start(&fs).await;
    let mut root = admin(&server).await;
    let taken = alter(
        &mut root,
        vec![],
        vec![upsert("admin", Kind::Sha256, "evil")],
    )
    .await;
    assert_eq!(taken, vec![("admin".to_owned(), 93)]);
    let listed: DescribeUserScramCredentialsResponse = root
        .call(
            ApiKey::DescribeUserScramCredentials,
            0,
            &DescribeUserScramCredentialsRequest::default(),
        )
        .await
        .unwrap();
    let mechanisms: Vec<_> = listed.results[0]
        .credential_infos
        .iter()
        .map(|i| (i.mechanism, i.iterations))
        .collect();
    assert_eq!(
        (listed.results[0].user.as_str(), mechanisms),
        ("admin", vec![(1, 4096), (2, 4096)])
    );
    refused(&server, Kind::Sha256, "admin", "evil").await;
    server.shutdown().await;
}

#[tokio::test]
async fn the_two_apis_are_advertised_after_login() {
    let server = start(&SimFs::new()).await;
    let mut root = admin(&server).await;
    let versions = root.versions().await.unwrap();
    let keys: Vec<_> = versions.api_keys.iter().map(|a| a.api_key).collect();
    assert!(keys.contains(&50) && keys.contains(&51), "{keys:?}");
    server.shutdown().await;
}
