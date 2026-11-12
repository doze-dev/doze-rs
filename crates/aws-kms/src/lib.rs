//! KMS: keys of the symmetric, HMAC, elliptic-curve and Ed25519 kinds, aliases, tags, key policies, rotation, encryption with
//! encryption contexts and data keys, signing and MACs, as a state machine behind a lazy actor. State is on disk and the key
//! material is sealed under the service's own key. Grants are kept and listed (not yet evaluated). Imported key material,
//! multi-region replicas, RSA keys and custom key stores are refused by name until they are built.

mod alias;
mod asym;
mod cipher;
mod core;
mod crypto;
mod find;
mod grants;
mod keys;
mod model;
mod policy;
mod rotation;
mod sign;
mod spec;
mod tags;

use std::sync::Arc;

use aws_model_kms::Operation;
use aws_model_kms::meta::META;
use aws_model_kms::tables::TABLES;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Lazy};

use crate::core::{Answer, Core};

/// The KMS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Kms {
    actor: Lazy<Core>,
}

impl Kms {
    /// Registers KMS. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actor: Lazy::new("kms", ctx),
        }
    }

    /// Asks the actor, and holds the answer until what it changed is on disk.
    async fn ask(&self, call: Call) -> Reply {
        let mut answer = match self.actor.call(call).await {
            Ok(answer) => answer,
            Err(e) => Answer {
                reply: Err(ApiError::internal(e.to_string())),
                durable: None,
            },
        };
        if let Some(saved) = answer.durable.take()
            && let Err(e) = saved.await
        {
            answer.reply = Err(ApiError::internal(format!("could not save: {e}")));
        }
        answer.reply
    }
}

impl Service for Kms {
    fn meta(&self) -> &'static aws_schema::meta::ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static aws_schema::tables::Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some_and(core::built)
    }

    fn flat_client_errors(&self) -> bool {
        true
    }

    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(self.ask(call))
    }
}

/// Whether `text` is a key policy this service accepts. Exists so the check can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_key_policy(text: &str) -> bool {
    policy::is_policy(text)
}

#[cfg(test)]
mod tests;
