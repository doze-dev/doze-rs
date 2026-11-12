//! AlterUserScramCredentials (API 51): set and delete SCRAM credentials at run time.
//!
//! The client computes the salted password, so the broker does no PBKDF2: it derives the two keys, writes them to the
//! table in one transaction and then swaps the new snapshot in, so a login begun after the answer sees the change and a
//! connection already logged in is not touched (Kafka re-checks only at re-authentication, which is not done here).

use std::collections::BTreeSet;

use kafka_auth::authz::{CLUSTER_NAME, operation, resource};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::api::Response;
use kafka_wire::messages::alter_user_scram_credentials_response::AlterUserScramCredentialsResult;
use kafka_wire::messages::{AlterUserScramCredentialsRequest, AlterUserScramCredentialsResponse};
use tracing::error;

use crate::users::row_of;
use crate::{Broker, Caller, Outcome, blocking};
use plan::{Failure, Plan, plan};

mod plan;

fn line(user: &str, code: i16, message: Option<&str>) -> AlterUserScramCredentialsResult {
    let mut r = AlterUserScramCredentialsResult::default();
    r.user = StrBytes::from_string(user.to_owned());
    r.error_code = code;
    r.error_message = message.map(|m| StrBytes::from_string(m.to_owned()));
    r
}

impl<F: Fs> Broker<F> {
    /// Applies the request as `ScramControlManager.alterCredentials` does, and answers with the deletions that
    /// happened, then the upsertions that happened, then the refusals.
    ///
    /// The caller needs ALTER on the cluster; without it every user named, once, sorted, is answered 31 and nothing is
    /// done (the shape of `AlterUserScramCredentialsRequest.getErrorResponse`, without its long message: SEC-D8). The
    /// whole request is one transaction, held under the lock that serialises changes, so two requests never start
    /// from the same snapshot. If the write fails, every operation that would have happened is answered
    /// KAFKA_STORAGE_ERROR and nothing changes.
    pub(crate) async fn alter_user_scram_credentials(
        &self,
        caller: &Caller,
        req: &AlterUserScramCredentialsRequest,
    ) -> Outcome {
        let mut resp = AlterUserScramCredentialsResponse::default();
        let allowed = self
            .authz
            .allows(caller, resource::CLUSTER, CLUSTER_NAME, operation::ALTER);
        if !allowed {
            let names: BTreeSet<&str> = (req.deletions.iter().map(|d| d.name.as_str()))
                .chain(req.upsertions.iter().map(|u| u.name.as_str()))
                .collect();
            let code = ResponseError::ClusterAuthorizationFailed.code();
            resp.results = names.into_iter().map(|n| line(n, code, None)).collect();
            return Outcome::Respond(Response::AlterUserScramCredentials(resp));
        }
        let _one_change_at_a_time = self.user_admin.lock().await;
        let stored = self.users.stored();
        // A caller who is not a super user may not touch a super user's credential, whatever the ACLs say (SEC-190).
        let caller_is_super = self
            .config
            .super_users
            .contains(caller.principal().as_str());
        let plan = plan(
            req,
            |user| self.why_not_changed(user, caller_is_super),
            |kind, user| stored.verifier(kind, user).is_some(),
        );
        let outcome = self.apply(&plan, &stored);
        let done = match outcome {
            Ok(()) => 0,
            Err(()) => ResponseError::KafkaStorageError.code(),
        };
        let removed = plan.deletions.iter().map(|(user, _)| user);
        let set = plan.upsertions.iter().map(|(user, _)| user);
        resp.results = removed
            .chain(set)
            .map(|user| line(user, done, None))
            .chain(
                plan.failures
                    .iter()
                    .map(|(user, Failure { code, message })| line(user, *code, *message)),
            )
            .collect();
        Outcome::Respond(Response::AlterUserScramCredentials(resp))
    }

    /// Why `user` may not be changed by this caller, if it may not: configured at start (nobody may), or a super user
    /// (and the caller is not one). Kafka has neither rule (SEC-D50, SEC-D70).
    fn why_not_changed(&self, user: &str, caller_is_super: bool) -> Option<Failure> {
        if user == kafka_auth::CONSOLE_USER {
            return Some(plan::RESERVED);
        }
        if self.users.is_bootstrap(user) {
            return Some(plan::CONFIGURED);
        }
        let is_super = self.config.super_users.contains(&format!("User:{user}"));
        (is_super && !caller_is_super).then_some(plan::SUPER_USER)
    }

    /// Writes the plan, then swaps the snapshot it makes. `Err` if the table could not be written, and then nothing is
    /// changed.
    fn apply(&self, plan: &Plan, stored: &kafka_auth::Stored) -> Result<(), ()> {
        if plan.deletions.is_empty() && plan.upsertions.is_empty() {
            return Ok(());
        }
        let removals: Vec<_> = (plan.deletions.iter())
            .map(|(user, kind)| kafka_meta::CredentialId {
                user: user.clone(),
                mechanism: crate::users::number_of(*kind),
            })
            .collect();
        let puts: Vec<_> = (plan.upsertions.iter())
            .map(|(user, verifier)| row_of(user, verifier))
            .collect();
        if let Err(e) = blocking::run(|| self.meta.alter_credentials(&puts, &removals)) {
            error!(error = %e, "storing SCRAM credentials failed");
            return Err(());
        }
        let mut next = stored.clone();
        for (user, kind) in &plan.deletions {
            next = next.without(user, *kind);
        }
        for (user, verifier) in &plan.upsertions {
            next = next.with(user, verifier.clone());
        }
        self.users.replace_stored(next);
        Ok(())
    }
}
