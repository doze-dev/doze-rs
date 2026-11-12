//! IAM and STS: the identities of an account (users, groups, roles, policies, keys, profiles, providers) and the sessions roles give
//! out. Two services on the wire (they speak the Query protocol, and have their own models and signing names) over one body of state
//! behind one lazy actor, kept on disk.
//!
//! **Soft mode only so far:** policies are stored, checked and returned, sessions are made and recognised, and nothing is denied.
//! Enforcement (a policy evaluator in front of every service) is the next piece.

mod account;
mod attached;
mod authz;
mod core;
mod entity;
mod errors;
mod gather;
mod groups;
mod keys;
mod managed;
mod model;
mod ops;
mod profiles;
mod providers;
mod req;
mod session;
mod simulate;
mod sts;
mod view;

use std::sync::Arc;

use aws_peers::{Authorizer, Need};
use aws_schema::json::Violation;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Caller, Reply, Service};
use doze_runtime::{Ctx, Lazy};

use crate::core::{Answer, Ask, Core, Which};

/// The state both services share, behind one actor.
#[derive(Debug)]
struct Shared {
    actor: Lazy<Core>,
}

impl Shared {
    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, which: Which, call: Call) -> Reply {
        let (mut reply, durable) = match self.actor.call(Ask::Call { which, call }).await {
            Ok(Answer::Call { reply, durable }) => (reply, durable),
            Ok(Answer::Authorize(_)) => (
                Err(ApiError::internal("IAM answered a call with a verdict")),
                None,
            ),
            Err(e) => (Err(ApiError::internal(e.to_string())), None),
        };
        if let Some(saved) = durable
            && let Err(e) = saved.await
        {
            reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        reply
    }

    /// Asks the actor whether `caller` may do `needs`.
    async fn authorize(&self, caller: &Caller, needs: Vec<Need>) -> Result<(), ApiError> {
        match self
            .actor
            .call(Ask::Authorize {
                caller: caller.clone(),
                needs,
            })
            .await
        {
            Ok(Answer::Authorize(verdict)) => verdict,
            Ok(Answer::Call { .. }) => {
                Err(ApiError::internal("IAM answered a question with a reply"))
            }
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }
}

impl Authorizer for Iam {
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(self.0.authorize(caller, needs))
    }
}

/// The IAM service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Iam(Arc<Shared>);

/// The STS service, over the same state as [`Iam`].
#[derive(Debug)]
pub struct Sts(Arc<Shared>);

/// IAM and STS over one body of state. `ctx` says where its files live and how it reads the time.
#[must_use]
pub fn services(ctx: Arc<Ctx>) -> (Iam, Sts) {
    let shared = Arc::new(Shared {
        actor: Lazy::new("iam", ctx),
    });
    (Iam(Arc::clone(&shared)), Sts(shared))
}

/// The words IAM and STS give a request that fails its model's checks: the Query services' `ValidationError`, and for a name that
/// breaks the rule, IAM's own sentence (recorded 2026-11-09).
fn invalid(tables: &'static aws_schema::tables::Tables, violation: &Violation) -> ApiError {
    if let Violation::Pattern {
        member,
        value: Some(_),
    } = violation
        && let Some(m) = tables.member(*member)
        && m.name.ends_with("Name")
        && !m.name.contains("Session")
    {
        let mut name = m.name.to_owned();
        if let Some(first) = name.get_mut(..1) {
            first.make_ascii_lowercase();
        }
        return ApiError::new(
            400,
            "ValidationError",
            format!(
                "The specified value for {name} is invalid. It must contain only alphanumeric characters and/or the following: +=,.@_-"
            ),
        );
    }
    let mut e = aws_wire::service::default_invalid(tables, violation);
    e.code = "ValidationError".into();
    e.shape = None;
    e
}

impl Service for Iam {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &aws_model_iam::meta::META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &aws_model_iam::tables::TABLES
    }

    fn serves(&self, op: u32) -> bool {
        aws_model_iam::Operation::from_index(op).is_some_and(|o| ops::BUILT.contains(&o))
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.0.ask(Which::Iam, call))
    }

    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(self.tables(), violation)
    }
}

impl Service for Sts {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &aws_model_sts::meta::META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &aws_model_sts::tables::TABLES
    }

    fn serves(&self, op: u32) -> bool {
        use aws_model_sts::Operation as O;
        matches!(
            O::from_index(op),
            Some(
                O::GetCallerIdentity
                    | O::AssumeRole
                    | O::AssumeRoleWithWebIdentity
                    | O::AssumeRoleWithSAML
                    | O::GetSessionToken
                    | O::GetFederationToken
                    | O::GetAccessKeyInfo
                    | O::DecodeAuthorizationMessage
            )
        )
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.0.ask(Which::Sts, call))
    }

    fn invalid(&self, violation: &Violation) -> ApiError {
        invalid(self.tables(), violation)
    }
}

#[cfg(test)]
mod tests;
