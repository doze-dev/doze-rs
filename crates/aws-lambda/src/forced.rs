//! Forced events (the console's Lab): the next invocation of a function is a cold one.
//!
//! A function runs in warm processes, one an invocation at a time, kept until they have been idle for a while. Lambda ends them
//! itself when a function's code or configuration changes, with [`Runtime::retire`], so that the next invocation starts one with the
//! new code: this is that call, for the function, with no change made to it. The next invocation then starts a process, which pays
//! for it and says so in its log line as `Init Duration`, as a cold start does in AWS. An invocation in progress is let finish.
//!
//! The authorizer is asked for `lambda:UpdateFunctionConfiguration` on the function, the right whose call ends its processes.
//!
//! [`Runtime::retire`]: crate::rt::Runtime

use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::forced::{self, Done, Forced, Step, kind};
use aws_wire::service::{BoxFuture, Caller};
use serde_json::Value;

use crate::core::{Answer, Ask};
use crate::resolve::Resolving;
use crate::{Lambda, arn};

impl Forced for Lambda {
    fn force<'a>(
        &'a self,
        caller: &'a Caller,
        what: &'a str,
        input: &'a Value,
    ) -> BoxFuture<'a, Result<Step, ApiError>> {
        Box::pin(async move {
            if what != kind::COLD_START {
                return Err(ApiError::internal(format!(
                    "Lambda has no forced event {what}"
                )));
            }
            let name =
                forced::text(input, "function").ok_or_else(|| forced::missing("a function"))?;
            self.allowed(caller, name).await?;
            self.cold(caller, name).await
        })
    }
}

impl Lambda {
    /// `lambda:UpdateFunctionConfiguration` on the function, as the call that retires its processes needs.
    async fn allowed(&self, caller: &Caller, name: &str) -> Result<(), ApiError> {
        let Some(peers) = self.inner.peers.clone().filter(|p| p.mode() != Mode::Off) else {
            return Ok(());
        };
        let need = Need::new(
            "lambda:UpdateFunctionConfiguration",
            arn::function(&caller.region, &caller.account_id, name, None),
        );
        peers.authorize(caller, vec![need]).await
    }

    /// Finds the function (a missing one is the call's own refusal), counts its processes, and retires them.
    async fn cold(&self, caller: &Caller, name: &str) -> Result<Step, ApiError> {
        let ask = Resolving {
            caller: caller.clone(),
            function: name.to_owned(),
            qualifier: None,
            source_arn: None,
            draw: 0.0,
            url: false,
            // Lambda itself looks, so that the right asked for is the one above, not the right to invoke.
            internal: true,
        };
        match self
            .inner
            .ask(&caller.region, Ask::Resolve(Box::new(ask)))
            .await?
        {
            Answer::Resolved(Ok(_)) => {}
            Answer::Resolved(Err(e)) => return Err(e),
            _ => return Err(ApiError::internal("Lambda answered with something else")),
        }
        let key = format!("{}/{name}", caller.region);
        let processes = self.inner.rt.alive(&key);
        if processes == 0 {
            return Ok(Step::Nothing(format!(
                "{name} has no process running in {}: it has not been invoked since its processes last ended (they end after {} idle, or when its code or configuration changes), so its next invocation is already a cold one.",
                caller.region,
                idle_words(self.inner.config.runtime.idle),
            )));
        }
        self.inner.rt.retire(&key);
        Ok(Step::Done(Done {
            sentence: format!(
                "Ended {} of {name}: its next invocation starts a new one and reports an Init Duration in its REPORT line, as a cold start does.{}",
                if processes == 1 {
                    "its 1 warm process".to_owned()
                } else {
                    format!("its {processes} warm processes")
                },
                " An invocation in progress is let finish first."
            ),
            subject: name.to_owned(),
        }))
    }
}

/// `10 minutes`, `90 seconds`.
fn idle_words(idle: std::time::Duration) -> String {
    let secs = idle.as_secs();
    if secs >= 120 && secs % 60 == 0 {
        format!("{} minutes", secs / 60)
    } else {
        format!("{secs} seconds")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::idle_words;

    #[test]
    fn an_idle_time_is_said_in_minutes_when_it_is_whole_ones() {
        assert_eq!(idle_words(Duration::from_secs(600)), "10 minutes");
        assert_eq!(idle_words(Duration::from_secs(90)), "90 seconds");
        assert_eq!(idle_words(Duration::from_secs(60)), "60 seconds");
    }
}
