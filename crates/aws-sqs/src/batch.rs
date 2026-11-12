//! What every batch operation shares: the checks on the batch as a whole, and how one entry's failure is written.
//! The messages are those AWS gave in `test_batch_refusals` and `test_batch_total_size` (recorded 2026-11-09).

use std::collections::HashSet;

use aws_model_sqs::keys::{
    BatchResultErrorEntry as Failed, ChangeMessageVisibilityBatchRequest as ChangeBatch,
    ChangeMessageVisibilityBatchRequestEntry as ChangeEntry,
    ChangeMessageVisibilityBatchResult as Changed,
    ChangeMessageVisibilityBatchResultEntry as ChangedEntry,
    DeleteMessageBatchRequest as DeleteBatch, DeleteMessageBatchRequestEntry as DeleteEntry,
    DeleteMessageBatchResult as Deleted, DeleteMessageBatchResultEntry as DeletedEntry,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, missing, reply, request};

/// The most entries in one batch.
pub(crate) const MAX_ENTRIES: usize = 10;
/// The most bytes in one batch of sends.
pub(crate) const MAX_BYTES: usize = 1_048_576;

fn batch_error(code: &'static str, message: String) -> ApiError {
    ApiError::new(400, format!("AWS.SimpleQueueService.{code}"), message)
}

/// The checks on a batch as a whole, made before any entry is tried: it is not empty, not too long, and its ids are
/// well formed and distinct. `entry_kind` names what an empty batch lacks (`SendMessageBatchRequestEntry`).
pub(crate) fn check(entry_kind: &str, ids: &[&str]) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Err(batch_error(
            "EmptyBatchRequest",
            format!("There should be at least one {entry_kind} in the request."),
        ));
    }
    if ids.len() > MAX_ENTRIES {
        return Err(batch_error(
            "TooManyEntriesInBatchRequest",
            format!(
                "Maximum number of entries per request are {MAX_ENTRIES}. You have sent {}.",
                ids.len()
            ),
        ));
    }
    let mut seen = HashSet::new();
    for id in ids {
        if !valid_id(id) {
            return Err(batch_error(
                "InvalidBatchEntryId",
                "A batch entry id can only contain alphanumeric characters, hyphens and underscores. It can be at most 80 letters long."
                    .to_owned(),
            ));
        }
        if !seen.insert(*id) {
            return Err(batch_error(
                "BatchEntryIdsNotDistinct",
                format!("Id {id} repeated."),
            ));
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    (1..=80).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Writes one failed entry: the entry's id, the code, who is at fault, and the words.
pub(crate) fn write_failed(e: &mut Object<'_, Failed>, id: &str, err: &ApiError) {
    e.put(Failed::ID, id)
        .put_bool(Failed::SENDER_FAULT, err.status < 500)
        .put(Failed::CODE, &err.code)
        .put(Failed::MESSAGE, &err.message);
}

impl Core {
    pub(crate) fn delete_message_batch(&mut self, call: &Call) -> Reply {
        let v = request::<DeleteBatch>(call)?;
        let queue_url = v
            .get(DeleteBatch::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let entries: Vec<_> = v.get(DeleteBatch::ENTRIES).into_iter().flatten().collect();
        let ids: Vec<&str> = entries
            .iter()
            .map(|e| e.get(DeleteEntry::ID).unwrap_or(""))
            .collect();
        check("DeleteMessageBatchRequestEntry", &ids)?;
        let queue = self.queue_mut(call, queue_url)?;
        let results: Vec<(&str, Result<(), ApiError>)> = entries
            .iter()
            .zip(&ids)
            .map(|(e, id)| {
                (
                    *id,
                    queue.delete_handle(e.get(DeleteEntry::RECEIPT_HANDLE).unwrap_or("")),
                )
            })
            .collect();
        reply::<Deleted>(|o| {
            if results.iter().any(|(_, r)| r.is_ok()) {
                o.put_list(Deleted::SUCCESSFUL, |l| {
                    for (id, _) in results.iter().filter(|(_, r)| r.is_ok()) {
                        l.push(|e| {
                            e.put(DeletedEntry::ID, id);
                        });
                    }
                });
            }
            write_failures(o, Deleted::FAILED, &results);
        })
    }

    pub(crate) fn change_message_visibility_batch(&mut self, call: &Call) -> Reply {
        let v = request::<ChangeBatch>(call)?;
        let queue_url = v
            .get(ChangeBatch::QUEUE_URL)
            .ok_or_else(|| missing("QueueUrl"))?;
        let entries: Vec<_> = v.get(ChangeBatch::ENTRIES).into_iter().flatten().collect();
        let ids: Vec<&str> = entries
            .iter()
            .map(|e| e.get(ChangeEntry::ID).unwrap_or(""))
            .collect();
        check("ChangeMessageVisibilityBatchRequestEntry", &ids)?;
        let now = self.now_ms();
        let queue = self.queue_mut(call, queue_url)?;
        let results: Vec<(&str, Result<(), ApiError>)> = entries
            .iter()
            .zip(&ids)
            .map(|(e, id)| {
                let handle = e.get(ChangeEntry::RECEIPT_HANDLE).unwrap_or("");
                let timeout = e.get(ChangeEntry::VISIBILITY_TIMEOUT).unwrap_or(0);
                (*id, queue.change_visibility(handle, timeout, now))
            })
            .collect();
        reply::<Changed>(|o| {
            if results.iter().any(|(_, r)| r.is_ok()) {
                o.put_list(Changed::SUCCESSFUL, |l| {
                    for (id, _) in results.iter().filter(|(_, r)| r.is_ok()) {
                        l.push(|e| {
                            e.put(ChangedEntry::ID, id);
                        });
                    }
                });
            }
            write_failures(o, Changed::FAILED, &results);
        })
    }
}

/// Writes the `Failed` list of a batch answer, if any entry failed.
fn write_failures<S>(
    o: &mut Object<'_, S>,
    key: aws_schema::view::Key<
        S,
        aws_schema::view::kind::List<aws_schema::view::kind::Obj<Failed>>,
    >,
    results: &[(&str, Result<(), ApiError>)],
) {
    if results.iter().any(|(_, r)| r.is_err()) {
        o.put_list(key, |l| {
            for (id, err) in results
                .iter()
                .filter_map(|(id, r)| r.as_ref().err().map(|e| (id, e)))
            {
                l.push(|e| write_failed(e, id, err));
            }
        });
    }
}
