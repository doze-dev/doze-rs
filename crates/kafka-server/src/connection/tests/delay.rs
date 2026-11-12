//! The delay before a failed login is answered (`connection.failed.authentication.delay.ms`, SEC-192): every path that
//! answers SASL_AUTHENTICATION_FAILED waits for it, the closes without an answer and the other errors do not, and
//! a stop does not wait for it.

use kafka_auth::scram::Kind;
use kafka_auth::scram::client::Client;
use tokio::time::timeout;

use super::login::{self, read_token_answer, start_login};
use super::scram::{self, challenge, read_frame, start_scram};
use super::*;

const DELAY: Duration = Duration::from_millis(100);

fn delayed(by: Duration) -> Timeouts {
    Timeouts {
        failed_login: by,
        ..Timeouts::default()
    }
}

async fn plain_handshake(client: &mut DuplexStream) {
    client.write_all(&login::handshake(1)).await.unwrap();
    read_frame(client).await;
}

#[test]
fn the_delay_is_kafkas_hundred_milliseconds_unless_set() {
    assert_eq!(Timeouts::default().failed_login, DELAY);
}

#[tokio::test(start_paused = true)]
async fn a_failed_plain_login_is_answered_after_the_delay_and_not_before() {
    let (mut client, task, _stop) = start_login(delayed(DELAY));
    plain_handshake(&mut client).await;
    let begun = Instant::now();
    client
        .write_all(&login::token(2, b"\0alice\0nope"))
        .await
        .unwrap();
    let early = timeout(Duration::from_millis(99), read_token_answer(&mut client)).await;
    assert!(early.is_err(), "answered before the delay was over");
    assert_eq!(read_token_answer(&mut client).await, 58);
    assert!(begun.elapsed() >= DELAY, "{:?}", begun.elapsed());
    // Flushed, then closed.
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
    assert!(matches!(task.await.unwrap(), End::LoginFailed));
}

#[tokio::test(start_paused = true)]
async fn a_delay_of_zero_answers_at_once() {
    let (mut client, _task, _stop) = start_login(delayed(Duration::ZERO));
    plain_handshake(&mut client).await;
    let begun = Instant::now();
    client
        .write_all(&login::token(2, b"\0alice\0nope"))
        .await
        .unwrap();
    assert_eq!(read_token_answer(&mut client).await, 58);
    assert!(begun.elapsed() < Duration::from_millis(10));
}

#[tokio::test(start_paused = true)]
async fn every_scram_refusal_that_is_answered_waits_and_the_silent_close_does_not() {
    // A wrong password (the final message), an unknown user (the first), and a proof the wrong length (no answer).
    let (mut client, task, _stop) = start_scram(delayed(DELAY));
    let me = Client::new(Kind::Sha256, "alice", "nope", "client-nonce.");
    let first = challenge(&mut client, Kind::Sha256, &me).await;
    let begun = Instant::now();
    let last = me.last(&first.auth_bytes, None, "");
    client
        .write_all(&scram::token(3, &last.message))
        .await
        .unwrap();
    assert_eq!(scram::read_token(&mut client).await.error_code, 58);
    assert!(begun.elapsed() >= DELAY);
    assert!(matches!(task.await.unwrap(), End::LoginFailed));

    let (mut client, task, _stop) = start_scram(delayed(DELAY));
    let ghost = Client::new(Kind::Sha256, "nobody", "pw", "client-nonce.");
    let begun = Instant::now();
    assert_eq!(
        challenge(&mut client, Kind::Sha256, &ghost)
            .await
            .error_code,
        58
    );
    assert!(begun.elapsed() >= DELAY);
    assert!(matches!(task.await.unwrap(), End::LoginFailed));

    let (mut client, task, _stop) = start_scram(delayed(Duration::from_secs(5)));
    let me = Client::new(Kind::Sha512, "alice", "pw", "client-nonce.");
    let first = challenge(&mut client, Kind::Sha512, &me).await;
    let text = String::from_utf8(first.auth_bytes.to_vec()).unwrap();
    let last = format!("c=biws,{},p=AAAA", text.split(',').next().unwrap());
    let begun = Instant::now();
    client
        .write_all(&scram::token(3, last.as_bytes()))
        .await
        .unwrap();
    assert!(matches!(task.await.unwrap(), End::LoginAbandoned));
    assert!(
        begun.elapsed() < Duration::from_secs(1),
        "a silent close waited"
    );
}

#[tokio::test(start_paused = true)]
async fn what_is_not_a_failed_login_is_not_delayed() {
    let long = delayed(Duration::from_secs(5));
    // A mechanism that is not offered: answered and closed, at once.
    let (mut client, task, _stop) = start_scram(long);
    let begun = Instant::now();
    client
        .write_all(&scram::handshake(1, "SCRAM-SHA-1"))
        .await
        .unwrap();
    read_frame(&mut client).await;
    assert!(matches!(task.await.unwrap(), End::LoginFailed));
    // A success.
    let (mut client, _task, _stop) = start_login(long);
    plain_handshake(&mut client).await;
    client
        .write_all(&login::token(2, b"\0alice\0pw"))
        .await
        .unwrap();
    assert_eq!(read_token_answer(&mut client).await, 0);
    assert!(begun.elapsed() < Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn a_stop_during_the_delay_ends_the_connection_without_waiting_for_it() {
    let (mut client, task, stop) = start_login(delayed(Duration::from_secs(60)));
    plain_handshake(&mut client).await;
    let begun = Instant::now();
    client
        .write_all(&login::token(2, b"\0alice\0nope"))
        .await
        .unwrap();
    sleep(Duration::from_secs(1)).await;
    stop.cancel();
    assert!(matches!(task.await.unwrap(), End::Stopped));
    assert!(begun.elapsed() < Duration::from_secs(5));
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).await.unwrap();
    assert!(
        rest.is_empty(),
        "no answer is written to a stopped server's client"
    );
}
