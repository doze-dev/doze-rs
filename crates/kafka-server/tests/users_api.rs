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
#[path = "support/users_server.rs"]
mod server;

use client::{Client, metadata_for};
use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client as ScramClient;
use kafka_storage::sim::SimFs;
use kafka_wire::messages::{
    ApiKey, DescribeUserScramCredentialsRequest, DescribeUserScramCredentialsResponse,
    MetadataResponse,
};
use server::{admin, alter, deletion, refused, start, upsert};

#[tokio::test]
async fn a_user_made_by_an_admin_logs_in_at_once_and_after_a_restart() {
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
    server.shutdown().await;
}

#[tokio::test]
async fn a_deleted_credential_is_refused_at_the_next_login_and_an_open_connection_stays_logged_in()
{
    let server = start(&SimFs::new()).await;
    let mut root = admin(&server).await;
    for kind in [Kind::Sha256, Kind::Sha512] {
        let made = alter(&mut root, vec![], vec![upsert("carol", kind, "carol-pw")]).await;
        assert_eq!(made, vec![("carol".to_owned(), 0)]);
    }
    // New logins with the deleted mechanism are refused, the other mechanism still works, and the connection that
    // was open stays open (Kafka re-checks only at re-authentication).
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
