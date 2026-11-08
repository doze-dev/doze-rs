use super::*;
use crate::error::StorageError;
use kafka_wire::batch::TimestampType;

#[test]
fn a_ticket_is_durable_once_a_sync_covers_it() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let first = log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    assert!(!first.ticket.is_durable());
    log.sync().unwrap();
    assert!(first.ticket.is_durable());
    let second = log.append(&ones(1, 0), NOW, TimestampType::Create).unwrap();
    assert!(!second.ticket.is_durable() && first.ticket.is_durable());
    block_on(async {
        log.sync().unwrap();
        second.ticket.durable().await.unwrap();
    });
}

#[test]
fn one_sync_covers_every_append_before_it() {
    let (fs, _dir, mut log) = fresh(NEVER);
    log.sync().unwrap(); // the new partition's own names
    let before = fsyncs(&fs);
    let tickets: Vec<_> = (0..3)
        .map(|_| {
            log.append(&ones(2, 0), NOW, TimestampType::Create)
                .unwrap()
                .ticket
        })
        .collect();
    log.sync().unwrap();
    assert_eq!(
        fsyncs(&fs) - before,
        2,
        "the log and its index, once, for all three"
    );
    assert!(tickets.iter().all(Ticket::is_durable));
}

#[test]
fn a_ticket_does_not_wait_for_ever_for_a_log_that_is_gone() {
    let (_fs, _dir, mut log) = fresh(NEVER);
    let reader = log.reader(); // keeps the log's shared state alive, as a fetch in flight would
    let ticket = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .ticket;
    drop(log);
    let outcome = block_on(ticket.durable());
    assert!(
        matches!(outcome, Err(StorageError::Offline { .. })),
        "{outcome:?}"
    );
    drop(reader);
    // A ticket that was already satisfied stays satisfied after the log is gone.
    let (_fs, _dir, mut log) = fresh(NEVER);
    let ticket = log
        .append(&ones(1, 0), NOW, TimestampType::Create)
        .unwrap()
        .ticket;
    log.sync().unwrap();
    let _keep = log.reader();
    drop(log);
    block_on(ticket.durable()).unwrap();
}
