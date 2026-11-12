//! A connection that logs in with SCRAM, over an in-memory pipe: both rounds are inside the login budget,
//! a stranger's body is still not decoded between them, a refusal is flushed before the close, and a proof of the
//! wrong length is closed with nothing written.

use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client;
use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_wire::messages::{
    MetadataRequest, SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
};

use super::*;

fn needing_scram(timeouts: Timeouts) -> Context<SimFs> {
    let users = [("alice".to_owned(), "pw".to_owned())];
    let all = [
        Mechanism::Plain,
        Mechanism::ScramSha512,
        Mechanism::ScramSha256,
    ];
    let auth = Authenticator::new(&all, Credentials::new(users).unwrap()).unwrap();
    let mut ctx = context(timeouts);
    ctx.features.sasl = true;
    ctx.features.served = ctx
        .features
        .served
        .with(ApiKey::SaslHandshake)
        .with(ApiKey::SaslAuthenticate);
    ctx.auth = Some(Arc::new(auth));
    ctx
}

pub(super) fn start_scram(
    timeouts: Timeouts,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<End>,
    CancellationToken,
) {
    let (client, server) = duplex(1 << 20);
    let stop = CancellationToken::new();
    let ctx = needing_scram(timeouts);
    let task = tokio::spawn({
        let stop = stop.clone();
        async move { serve(&mut { server }, &ctx, None, Instant::now(), &stop).await }
    });
    (client, task, stop)
}

pub(super) fn handshake(id: i32, mechanism: &'static str) -> Bytes {
    let mut body = SaslHandshakeRequest::default();
    body.mechanism = StrBytes::from_static_str(mechanism);
    framed(&request_payload(ApiKey::SaslHandshake, 1, id, None, &body))
}

pub(super) fn token(id: i32, bytes: &[u8]) -> Bytes {
    let mut body = SaslAuthenticateRequest::default();
    body.auth_bytes = Bytes::copy_from_slice(bytes);
    framed(&request_payload(
        ApiKey::SaslAuthenticate,
        2,
        id,
        None,
        &body,
    ))
}

pub(super) async fn read_frame(client: &mut DuplexStream) -> Bytes {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut payload).await.unwrap();
    Bytes::from(payload)
}

pub(super) async fn read_token(client: &mut DuplexStream) -> SaslAuthenticateResponse {
    let mut payload = read_frame(client).await;
    let _id_and_tags = payload.split_to(5);
    SaslAuthenticateResponse::decode(&mut payload, 2).unwrap()
}

/// The handshake and the first token: the server-first message.
pub(super) async fn challenge(
    client: &mut DuplexStream,
    kind: Kind,
    me: &Client,
) -> SaslAuthenticateResponse {
    client
        .write_all(&handshake(1, kind.mechanism().name()))
        .await
        .unwrap();
    read_frame(client).await;
    client.write_all(&token(2, &me.first())).await.unwrap();
    read_token(client).await
}

#[tokio::test]
async fn a_scram_login_takes_two_tokens_and_then_the_connection_is_served() {
    for kind in Kind::ALL {
        let (mut client, task, _stop) = start_scram(Timeouts::default());
        let me = Client::new(kind, "alice", "pw", "client-nonce.");
        let first = challenge(&mut client, kind, &me).await;
        assert_eq!(first.error_code, 0);
        let last = me.last(&first.auth_bytes, None, "");
        client.write_all(&token(3, &last.message)).await.unwrap();
        let done = read_token(&mut client).await;
        assert_eq!(done.error_code, 0);
        assert_eq!(done.auth_bytes.as_ref(), last.expected.as_slice());
        // Logged in: the stranger's frame cap and the login budget are gone, and APIs are served.
        client.write_all(&big_produce(7, 600_000)).await.unwrap();
        assert_eq!(read_correlation(&mut client).await, 7);
        assert!(!task.is_finished());
    }
}

#[tokio::test]
async fn a_wrong_password_is_flushed_before_the_close() {
    let (mut client, task, _stop) = start_scram(Timeouts::default());
    let me = Client::new(Kind::Sha256, "alice", "nope", "client-nonce.");
    let first = challenge(&mut client, Kind::Sha256, &me).await;
    let last = me.last(&first.auth_bytes, None, "");
    client.write_all(&token(3, &last.message)).await.unwrap();
    let refused = read_token(&mut client).await;
    assert_eq!(refused.error_code, 58);
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
    assert!(matches!(task.await.unwrap(), End::LoginFailed));
}

#[tokio::test]
async fn a_proof_the_wrong_length_ends_the_connection_with_nothing_written() {
    let (mut client, task, _stop) = start_scram(Timeouts::default());
    let me = Client::new(Kind::Sha512, "alice", "pw", "client-nonce.");
    let first = challenge(&mut client, Kind::Sha512, &me).await;
    let text = String::from_utf8(first.auth_bytes.to_vec()).unwrap();
    let last = format!("c=biws,{},p=AAAA", text.split(',').next().unwrap());
    client.write_all(&token(3, last.as_bytes())).await.unwrap();
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty(), "no answer: {rest:?}");
    assert!(matches!(task.await.unwrap(), End::LoginAbandoned));
}

#[tokio::test]
async fn between_the_two_tokens_a_strangers_body_is_still_not_decoded() {
    let (mut client, task, _stop) = start_scram(Timeouts::default());
    let me = Client::new(Kind::Sha256, "alice", "pw", "client-nonce.");
    challenge(&mut client, Kind::Sha256, &me).await;
    let body = MetadataRequest::default();
    let good = request_payload(ApiKey::Metadata, 12, 3, None, &body);
    let mut payload = good.to_vec();
    payload.extend_from_slice(&[0xff; 40]);
    client.write_all(&framed(&payload)).await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::OutOfOrder(ApiKey::Metadata)
    ));
}

#[tokio::test(start_paused = true)]
async fn the_login_budget_covers_both_rounds() {
    let timeouts = Timeouts {
        pre_auth: Duration::from_secs(30),
        ..Timeouts::default()
    };
    let (mut client, task, _stop) = start_scram(timeouts);
    let me = Client::new(Kind::Sha256, "alice", "pw", "client-nonce.");
    challenge(&mut client, Kind::Sha256, &me).await;
    sleep(Duration::from_secs(25)).await;
    assert!(!task.is_finished());
    assert!(matches!(task.await.unwrap(), End::LoginTooSlow));
}
