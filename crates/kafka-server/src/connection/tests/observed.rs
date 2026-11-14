//! An installed observer is told of every connection and request, in order, whatever becomes of the request; one that is
//! not installed changes nothing; and one that panics takes down its own connection and no other.

use std::sync::Mutex;

use kafka_wire::messages::{MetadataRequest, ProduceRequest};

use super::login::{login, needing_login};
use super::*;
use crate::observer::{Answer, Asked, Call, ConnId, Observed, Observer, Opened, Pending};

/// Writes down what it is told, as short lines.
#[derive(Debug, Default)]
struct Tape(Mutex<Vec<String>>);

impl Tape {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn put(&self, line: String) {
        self.0.lock().unwrap().push(line);
    }
}

impl Observer for Tape {
    fn opened(&self, conn: &Opened) {
        self.put(format!("opened {}", conn.id.0));
    }
    fn begin(&self, call: &Call<'_>) -> Pending {
        let what = match call.asked {
            Asked::Request(r) => format!("{:?}", r.api_key()),
            Asked::ApiVersions(Some(b)) => format!("ApiVersions({})", b.client_software_name),
            Asked::ApiVersions(None) => "ApiVersions".into(),
            Asked::Undecoded => format!("{:?} undecoded", call.head.key),
        };
        self.put(format!(
            "begin {} {what} as {} ({} B)",
            call.head.correlation_id,
            call.principal.as_str(),
            call.request_bytes
        ));
        Pending::new(call.head.correlation_id)
    }
    fn end(&self, pending: Pending, seen: &Observed<'_>) {
        let id = pending.into_payload::<i32>().unwrap();
        let how = match seen.answer {
            Answer::Response(r) => format!("answered {:?}", r.api_key()),
            Answer::Nothing => "nothing".into(),
            Answer::Closed(why) => format!("closed: {why}"),
        };
        self.put(format!("end {id} {how} ({} B)", seen.response_bytes));
    }
    fn closed(&self, conn: ConnId, why: &str) {
        self.put(format!("closed {}: {why}", conn.0));
    }
}

fn watched(mut ctx: Context<SimFs>, observer: Arc<dyn Observer>) -> Context<SimFs> {
    ctx.observer = Some(observer);
    ctx
}

fn serving(
    ctx: Context<SimFs>,
) -> (
    DuplexStream,
    tokio::task::JoinHandle<End>,
    CancellationToken,
) {
    let (client, server) = duplex(1 << 20);
    let stop = CancellationToken::new();
    let task = tokio::spawn({
        let stop = stop.clone();
        async move { serve(&mut { server }, &ctx, None, Instant::now(), &stop).await }
    });
    (client, task, stop)
}

fn metadata(id: i32) -> Bytes {
    framed(&request_payload(
        ApiKey::Metadata,
        12,
        id,
        Some("t"),
        &MetadataRequest::default(),
    ))
}

async fn read_frame(client: &mut DuplexStream) -> usize {
    let mut len = [0; 4];
    client.read_exact(&mut len).await.unwrap();
    let n = u32::from_be_bytes(len) as usize;
    let mut payload = vec![0; n];
    client.read_exact(&mut payload).await.unwrap();
    n + 4
}

#[tokio::test]
async fn each_request_is_told_twice_in_order_between_the_open_and_the_close() {
    let tape = Arc::new(Tape::default());
    let (mut client, task, _stop) = serving(watched(context(Timeouts::default()), tape.clone()));
    client.write_all(&api_versions(1)).await.unwrap();
    let first = read_frame(&mut client).await;
    client.write_all(&metadata(2)).await.unwrap();
    let second = read_frame(&mut client).await;
    drop(client);
    assert!(matches!(task.await.unwrap(), End::PeerClosed));
    let lines = tape.lines();
    assert_eq!(lines[0], "opened 1");
    assert!(
        lines[1].starts_with("begin 1 ApiVersions(test) as User:ANONYMOUS"),
        "{lines:?}"
    );
    assert_eq!(lines[2], format!("end 1 answered ApiVersions ({first} B)"));
    assert!(lines[3].starts_with("begin 2 Metadata as User:ANONYMOUS"));
    assert_eq!(lines[4], format!("end 2 answered Metadata ({second} B)"));
    assert_eq!(lines[5], "closed 1: the client closed the connection");
    assert_eq!(lines.len(), 6);
}

#[tokio::test]
async fn a_request_the_connection_is_closed_for_is_told_as_closed_and_a_login_changes_the_principal()
 {
    let tape = Arc::new(Tape::default());
    // A stranger asks for Metadata before logging in: closed, and told so.
    let (mut client, task, _stop) =
        serving(watched(needing_login(Timeouts::default()), tape.clone()));
    client.write_all(&metadata(1)).await.unwrap();
    assert!(matches!(
        task.await.unwrap(),
        End::OutOfOrder(ApiKey::Metadata)
    ));
    let lines = tape.lines();
    assert_eq!(
        lines[1],
        "begin 1 Metadata undecoded as User:ANONYMOUS (".to_owned()
            + &metadata(1).len().saturating_sub(4).to_string()
            + " B)"
    );
    assert!(lines[2].starts_with("end 1 closed: a Metadata request was made out of order"));
    assert!(lines[3].starts_with("closed 1:"));
    // After the login the same request is served, as alice.
    let tape = Arc::new(Tape::default());
    let (mut client, _task, _stop) =
        serving(watched(needing_login(Timeouts::default()), tape.clone()));
    login(&mut client).await;
    client.write_all(&metadata(3)).await.unwrap();
    read_frame(&mut client).await;
    let lines = tape.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("begin 2 SaslAuthenticate as User:ANONYMOUS")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("begin 3 Metadata as User:alice")),
        "{lines:?}"
    );
}

#[tokio::test]
async fn a_request_with_no_answer_is_told_as_nothing() {
    let tape = Arc::new(Tape::default());
    let (mut client, _task, _stop) = serving(watched(context(Timeouts::default()), tape.clone()));
    let mut produce = ProduceRequest::default();
    produce.acks = 0;
    client
        .write_all(&framed(&request_payload(
            ApiKey::Produce,
            9,
            5,
            Some("t"),
            &produce,
        )))
        .await
        .unwrap();
    client.write_all(&api_versions(6)).await.unwrap();
    read_frame(&mut client).await;
    let lines = tape.lines();
    assert!(
        lines.contains(&"end 5 nothing (0 B)".to_owned()),
        "{lines:?}"
    );
}

/// Panics on the first request it is shown.
#[derive(Debug)]
struct Bomb;

impl Observer for Bomb {
    fn opened(&self, _: &Opened) {}
    fn begin(&self, _: &Call<'_>) -> Pending {
        panic!("an observer that must never be written");
    }
    fn end(&self, _: Pending, _: &Observed<'_>) {}
    fn closed(&self, _: ConnId, _: &str) {}
}

#[tokio::test]
async fn an_observer_that_panics_ends_its_own_connection_and_no_other() {
    // In a test build a panic ends the task of the connection it happened on; a release build aborts, so observers are
    // written not to panic (the recorder's lints forbid unwrap, expect, indexing and panic).
    let ctx = Arc::new(watched(context(Timeouts::default()), Arc::new(Bomb)));
    let other = Arc::new(context(Timeouts::default()));
    let spawn = |ctx: Arc<Context<SimFs>>| {
        let (client, server) = duplex(4096);
        let task = tokio::spawn(async move {
            serve(
                &mut { server },
                &ctx,
                None,
                Instant::now(),
                &CancellationToken::new(),
            )
            .await
        });
        (client, task)
    };
    let (mut bad, bad_task) = spawn(ctx);
    bad.write_all(&api_versions(1)).await.unwrap();
    assert!(bad_task.await.unwrap_err().is_panic());
    let (mut good, _good_task) = spawn(other);
    good.write_all(&api_versions(2)).await.unwrap();
    assert_eq!(read_answer(&mut good).await.0, 2);
}
