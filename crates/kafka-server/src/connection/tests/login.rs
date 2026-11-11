//! A connection that has to log in: what a stranger may do, what it costs, and when it is let go.

use kafka_auth::{Authenticator, Credentials, Mechanism};
use kafka_wire::messages::{
    MetadataRequest, SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest,
};

use super::*;

/// A context that wants a login from alice (password "pw").
fn needing_login(timeouts: Timeouts) -> Context<SimFs> {
    let users = [("alice".to_owned(), "pw".to_owned())];
    let auth = Authenticator::new(&[Mechanism::Plain], Credentials::new(users).unwrap()).unwrap();
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

fn start_login(
    timeouts: Timeouts,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<End>,
    CancellationToken,
) {
    let (client, server) = duplex(1 << 20);
    let stop = CancellationToken::new();
    let ctx = needing_login(timeouts);
    let task = tokio::spawn({
        let stop = stop.clone();
        async move { serve(server, &ctx, None, &stop).await }
    });
    (client, task, stop)
}

fn handshake(id: i32) -> Bytes {
    let mut body = SaslHandshakeRequest::default();
    body.mechanism = StrBytes::from_static_str("PLAIN");
    framed(&request_payload(ApiKey::SaslHandshake, 1, id, None, &body))
}

fn token(id: i32, bytes: &'static [u8]) -> Bytes {
    let mut body = SaslAuthenticateRequest::default();
    body.auth_bytes = Bytes::from_static(bytes);
    framed(&request_payload(
        ApiKey::SaslAuthenticate,
        2,
        id,
        None,
        &body,
    ))
}

/// The answer to a SaslAuthenticate v2: its error code.
async fn read_token_answer(client: &mut DuplexStream) -> i16 {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut payload).await.unwrap();
    let mut payload = Bytes::from(payload);
    let _id_and_tags = payload.split_to(5);
    SaslAuthenticateResponse::decode(&mut payload, 2)
        .unwrap()
        .error_code
}

/// A Metadata v12 request whose body is not a body at all.
fn metadata_with_garbage_body(id: i32) -> Bytes {
    let body = MetadataRequest::default();
    let good = request_payload(ApiKey::Metadata, 12, id, None, &body);
    let mut encoded = bytes::BytesMut::new();
    kafka_protocol::protocol::Encodable::encode(&body, &mut encoded, 12).unwrap();
    let mut payload = good.slice(..good.len() - encoded.len()).to_vec();
    payload.extend_from_slice(&[0xff; 40]);
    framed(&payload)
}

async fn login(client: &mut DuplexStream) {
    client.write_all(&handshake(1)).await.unwrap();
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut rest = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut rest).await.unwrap();
    client.write_all(&token(2, b"\0alice\0pw")).await.unwrap();
    assert_eq!(read_token_answer(client).await, 0);
}

#[tokio::test]
async fn a_strangers_body_is_not_decoded_but_a_logged_in_clients_is() {
    // Before the login the verdict is on the header: out of order, not a body that failed to decode.
    let (mut client, task, _stop) = start_login(Timeouts::default());
    client
        .write_all(&metadata_with_garbage_body(1))
        .await
        .unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::OutOfOrder(ApiKey::Metadata)
    ));
    // After it, the same bytes are decoded, and refused as what they are.
    let (mut client, task, _stop) = start_login(Timeouts::default());
    login(&mut client).await;
    client
        .write_all(&metadata_with_garbage_body(3))
        .await
        .unwrap();
    assert!(matches!(task.await.unwrap(), End::Wire(_)));
}

#[tokio::test]
async fn a_failed_login_is_flushed_before_the_close_and_ends_the_connection() {
    let (mut client, task, _stop) = start_login(Timeouts::default());
    client.write_all(&handshake(1)).await.unwrap();
    client.write_all(&token(2, b"\0alice\0nope")).await.unwrap();
    let mut all = Vec::new();
    client.read_to_end(&mut all).await.unwrap();
    assert!(
        !all.is_empty(),
        "both answers were written before the close"
    );
    assert!(matches!(task.await.unwrap(), End::LoginFailed));
}

#[tokio::test]
async fn a_stranger_frame_is_held_to_512_kib_and_a_logged_in_one_is_not() {
    let (mut client, task, _stop) = start_login(Timeouts::default());
    client.write_all(&(600_000u32).to_be_bytes()).await.unwrap();
    client
        .write_all(&(ApiKey::ApiVersions as i16).to_be_bytes())
        .await
        .unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::Wire(WireError::FrameTooLarge { limit: 524_288, .. })
    ));
    let (mut client, _task, _stop) = start_login(Timeouts::default());
    login(&mut client).await;
    client.write_all(&big_produce(7, 600_000)).await.unwrap();
    assert_eq!(read_correlation(&mut client).await, 7);
}

#[tokio::test(start_paused = true)]
async fn a_connection_that_does_not_log_in_within_the_budget_is_closed_however_it_trickles() {
    let timeouts = Timeouts {
        pre_auth: Duration::from_secs(30),
        ..Timeouts::default()
    };
    // Silence: closed at 30 s, not at the 10 minutes of idleness.
    let (_client, task, _stop) = start_login(timeouts);
    assert!(matches!(task.await.unwrap(), End::LoginTooSlow));
    // A handshake every few seconds does not restart the clock.
    let (mut client, task, _stop) = start_login(timeouts);
    client.write_all(&api_versions(1)).await.unwrap();
    read_answer(&mut client).await;
    sleep(Duration::from_secs(25)).await;
    assert!(!task.is_finished());
    assert!(matches!(task.await.unwrap(), End::LoginTooSlow));
}

#[tokio::test(start_paused = true)]
async fn the_budget_ends_with_the_login() {
    let timeouts = Timeouts {
        pre_auth: Duration::from_secs(30),
        ..Timeouts::default()
    };
    let (mut client, task, _stop) = start_login(timeouts);
    sleep(Duration::from_secs(29)).await;
    login(&mut client).await;
    sleep(Duration::from_secs(120)).await;
    assert!(
        !task.is_finished(),
        "a logged in connection is bound by the idle limit only"
    );
    client.write_all(&api_versions(9)).await.unwrap();
    assert_eq!(read_answer(&mut client).await.0, 9);
}

#[tokio::test]
async fn without_sasl_a_handshake_is_not_a_request_the_server_offered() {
    let (mut client, task, _stop) = start(Timeouts::default(), 4096);
    client.write_all(&handshake(1)).await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::Wire(WireError::NotAdvertised {
            key: ApiKey::SaslHandshake,
            ..
        })
    ));
}

#[tokio::test]
async fn nothing_a_connection_ends_with_says_what_the_client_sent() {
    let (mut client, task, _stop) = start_login(Timeouts::default());
    client.write_all(&handshake(1)).await.unwrap();
    client
        .write_all(&token(2, b"\0alice\0planted-wrong"))
        .await
        .unwrap();
    let end = task.await.unwrap();
    assert!(!format!("{end} {end:?}").contains("planted"));
}

/// An ApiVersions request at a version nobody serves, as a client opening at its newest does: header v2 and no
/// body worth reading.
fn api_versions_at_v99(id: i32) -> Bytes {
    let mut payload = vec![0, 18, 0, 99];
    payload.extend_from_slice(&id.to_be_bytes());
    payload.extend_from_slice(&[0xff, 0xff, 0]); // no client id, no tagged fields
    framed(&payload)
}

/// The error code of the next answer, whatever it is (ApiVersions answers always have a v0 header and begin
/// with the error code).
async fn read_error_code(client: &mut DuplexStream) -> i16 {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let mut payload = vec![0; u32::from_be_bytes(len) as usize];
    client.read_exact(&mut payload).await.unwrap();
    i16::from_be_bytes([payload[4], payload[5]])
}

#[tokio::test]
async fn an_apiversions_answered_with_an_error_does_not_use_up_the_one_a_stranger_has() {
    let (mut client, task, _stop) = start_login(Timeouts::default());
    client.write_all(&api_versions_at_v99(1)).await.unwrap();
    assert_eq!(
        read_error_code(&mut client).await,
        35,
        "UNSUPPORTED_VERSION"
    );
    // The client asks again at a version that is served: answered, and that one is the one.
    client.write_all(&api_versions(2)).await.unwrap();
    assert_eq!(read_error_code(&mut client).await, 0);
    client.write_all(&api_versions(3)).await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::OutOfOrder(ApiKey::ApiVersions)
    ));
}
