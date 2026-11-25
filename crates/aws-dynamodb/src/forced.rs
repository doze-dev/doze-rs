//! Forced events (the console's Lab): a table's time-to-live sweep, now.
//!
//! The sweep is what the region's timer asks for: [`Request::Tick`] runs `housekeeping` with the once-a-second limit lifted, and this
//! runs the very same function, after counting what is expired in the table so that it can say what the sweep did. The sweep covers
//! every table of the region that has time to live, as the timer's does. doze deletes an item when it is due, so a sweep seldom finds
//! anything: with nothing expired it is refused in words, with when the next item expires (AWS's own deletion is far slower).
//!
//! The authorizer is asked for `dynamodb:DeleteItem` on the table: the sweep deletes items.

use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::forced::{self, Done, Forced, Step, kind};
use aws_wire::service::{BoxFuture, Caller};
use serde_json::Value;

use crate::Dynamodb;
use crate::core::{Core, Request};
use crate::meta::Status;
use crate::req::table_arn;

/// What a forced sweep found and did.
#[derive(Debug)]
pub(crate) enum Sweep {
    /// The table has no time to live on.
    Off,
    /// No item of the table has passed its time to live; when the first one will.
    Nothing { next_ms: Option<i64> },
    /// The sweep ran: items of the table removed, of the other tables, and what is still expired (a batch is limited, so a count
    /// stops at it).
    Ran {
        removed: usize,
        elsewhere: usize,
        left: usize,
        appended: bool,
    },
}

impl Core {
    /// Counts what is expired in table `name`, and when it is not nothing runs the timer's sweep.
    pub(crate) async fn sweep_now(&mut self, name: &str) -> Result<Sweep, ApiError> {
        let t = self.table(name)?;
        if !t.ttl.enabled || t.status == Status::Deleting {
            return Ok(Sweep::Off);
        }
        let now = self.clock.now_ms();
        let before = self.expired_keys(&t, now / 1000)?.len();
        if before == 0 {
            return Ok(Sweep::Nothing {
                next_ms: self.first_expiry_of(&t),
            });
        }
        let others = self.expired_elsewhere(name, now / 1000)?;
        // The timer's own sweep: not held back by the once-a-second limit.
        self.next_sweep_ms = 0;
        let appended = self.housekeeping().await?;
        // A count stops at a batch, which is also what a sweep removes of each table: what was counted is what went.
        let left = self.expired_keys(&t, now / 1000)?.len();
        Ok(Sweep::Ran {
            removed: before,
            elsewhere: others,
            left,
            appended,
        })
    }

    /// [`Core::sweep_now`], answered as the timer's tick is: with whether a stream got records and what is left to do.
    pub(crate) async fn sweep_answer(&mut self, table: &str) -> crate::core::Answer {
        let swept = self.sweep_now(table).await;
        let appended = matches!(&swept, Ok(Sweep::Ran { appended: true, .. }));
        let mut answer = crate::core::Answer::of(Err(ApiError::internal("not a call")), appended);
        answer.work = std::mem::take(&mut self.work);
        answer.swept = Some(swept);
        answer
    }

    /// How many items are expired in the tables with time to live other than `name`.
    fn expired_elsewhere(&self, name: &str, now_secs: i64) -> Result<usize, ApiError> {
        let mut total = 0;
        for (other, t) in self.tables.iter() {
            if other != name && t.ttl.enabled && t.status != Status::Deleting {
                total += self.expired_keys(t, now_secs)?.len();
            }
        }
        Ok(total)
    }
}

impl Forced for Dynamodb {
    fn force<'a>(
        &'a self,
        caller: &'a Caller,
        what: &'a str,
        input: &'a Value,
    ) -> BoxFuture<'a, Result<Step, ApiError>> {
        Box::pin(async move {
            if what != kind::SWEEP_TTL {
                return Err(ApiError::internal(format!(
                    "DynamoDB has no forced event {what}"
                )));
            }
            let table = forced::text(input, "table").ok_or_else(|| forced::missing("a table"))?;
            self.allowed(caller, table).await?;
            let sweep = self.sweep(&caller.region, table).await?;
            Ok(words(table, sweep))
        })
    }
}

impl Dynamodb {
    /// `dynamodb:DeleteItem` on the table, as a delete would need.
    async fn allowed(&self, caller: &Caller, table: &str) -> Result<(), ApiError> {
        let Some(peers) = self.peers.clone().filter(|p| p.mode() != Mode::Off) else {
            return Ok(());
        };
        let need = Need::new(
            "dynamodb:DeleteItem",
            table_arn(&caller.region, &caller.account_id, table),
        );
        peers.authorize(caller, vec![need]).await
    }

    /// Asks the region's actor for the sweep, and does what a call leaves to do after it: wakes consumers of a stream, re-arms the
    /// timer, and runs the work outside the actor.
    async fn sweep(&self, region: &str, table: &str) -> Result<Sweep, ApiError> {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let answer = actor
            .call(Request::Sweep {
                table: table.to_owned(),
            })
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if answer.appended {
            self.arrived.notify_waiters();
        }
        self.expiry.arm(region, &actor, answer.next_due);
        self.after(&actor, answer.work).await;
        answer
            .swept
            .unwrap_or_else(|| Err(ApiError::internal("the sweep gave no answer")))
    }
}

/// What the sweep came to, in words.
fn words(table: &str, sweep: Sweep) -> Step {
    const AWS: &str = "DynamoDB typically deletes expired items within two days of expiration, and until then they still show up in reads, queries and scans; doze deletes them when they are due.";
    match sweep {
        Sweep::Off => Step::Nothing(format!(
            "Time to live is off on {table}, so none of its items ever expires. Turn it on with UpdateTimeToLive: the attribute it names holds each item's expiry, in epoch seconds."
        )),
        Sweep::Nothing { next_ms } => {
            let next = next_ms.map_or_else(
                || "None of its items has an expiry set.".to_owned(),
                |ms| format!("The next item expires at {}.", utc(ms)),
            );
            Step::Nothing(format!(
                "No item of {table} has passed its time to live, so the sweep has nothing to remove. {next} {AWS}"
            ))
        }
        Sweep::Ran {
            removed,
            elsewhere,
            left,
            appended,
        } => {
            let mut s = format!(
                "Ran the time-to-live sweep: removed {} of {table}",
                items(removed)
            );
            if elsewhere > 0 {
                s.push_str(&format!(
                    " and {} of the region's other tables with time to live (the sweep covers all of them, as its timer's does)",
                    items(elsewhere)
                ));
            }
            s.push('.');
            if appended {
                s.push_str(" Each removal is a REMOVE record on the table's stream, written by the service, not by a user.");
            }
            if left > 0 {
                s.push_str(&format!(
                    " {} more {} still expired: a sweep removes one batch, and the timer carries on with the rest.",
                    left,
                    if left == 1 { "item is" } else { "items are" }
                ));
            }
            Step::Done(Done {
                sentence: s,
                subject: table.to_owned(),
            })
        }
    }
}

fn items(n: usize) -> String {
    if n == 1 {
        "1 expired item".to_owned()
    } else {
        format!("{n} expired items")
    }
}

/// `2026-01-02 09:00:05 UTC`.
fn utc(ms: i64) -> String {
    let iso = aws_core::time::format_iso8601(ms.div_euclid(1000));
    format!("{} UTC", iso.get(..19).unwrap_or(&iso).replace('T', " "))
}

/// [`words`], for the tests.
#[cfg(test)]
pub(crate) fn words_for_tests(table: &str, sweep: Sweep) -> Step {
    words(table, sweep)
}
