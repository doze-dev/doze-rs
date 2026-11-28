//! KMS: keys of the symmetric, HMAC, elliptic-curve, Ed25519 and RSA kinds, aliases, tags, key policies (evaluated by IAM as the key's
//! resource policy, which must name the caller or the account before an identity policy counts), rotation, imported key material,
//! encryption with encryption contexts and data keys and key pairs, signing and MACs, as a state machine behind a lazy actor. State is
//! on disk and the key material is sealed under the service's own key. Grants are kept and listed (not evaluated). ECDH key
//! agreement (`DeriveSharedSecret`), multi-Region keys with replicas in other regions (`multi.rs`) and the last use of a key are
//! built; custom key stores are refused by name.

mod agree;
mod alias;
mod asym;
mod authz;
mod cipher;
mod core;
mod crypto;
mod find;
mod grants;
mod import;
mod keys;
mod managed;
mod model;
mod multi;
mod offload;
mod policy;
mod rotation;
mod rsa_keys;
mod rsa_ops;
mod shown;
mod sign;
mod spec;
mod storage;
mod tags;

use std::sync::Arc;

use aws_model_kms::Operation;
use aws_model_kms::meta::META;
use aws_model_kms::tables::TABLES;
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use doze_runtime::{Ctx, Keyed};

use crate::core::{Answer, Core};

/// The region whose files are kept in the service's directory itself (state written before regions were kept apart is its).
const HOME_REGION: &str = "us-east-1";

/// The KMS service. Nothing runs until the first request.
#[derive(Debug)]
pub struct Kms {
    /// One actor per region: a queue, parameter, secret or key belongs to the region that made it.
    actors: Keyed<Core>,
}

impl Kms {
    /// Registers KMS. `ctx` says where its files live and how it reads the time.
    #[must_use]
    pub fn new(ctx: Arc<Ctx>) -> Self {
        Self {
            actors: Keyed::new("kms", ctx, HOME_REGION),
        }
    }

    /// Sends the call to the actor of `region`.
    async fn call_actor(&self, region: &str, ask: crate::core::Ask) -> Answer {
        let actor = match self.actors.actor(region) {
            Ok(actor) => actor,
            Err(e) => return Answer::of(Err(ApiError::internal(e.to_string()))),
        };
        actor
            .call(ask)
            .await
            .unwrap_or_else(|e| Answer::of(Err(ApiError::internal(e.to_string()))))
    }

    /// Asks the actor, and holds the answer until what it changed is on disk. Work the call leaves for off the event loop is done
    /// here, outside the actor (`offload.rs`): a job, after which the call is asked again with what it made, or the answer itself.
    async fn ask(&self, call: Call) -> Reply {
        use crate::core::Ask;
        use crate::offload::Next;
        let region = call.caller.region.clone();
        let mut ask = Ask::Call(Box::new(call));
        // A call asks for one job at most; a second asking that wants another (its key changed in between) is given one more go.
        for _ in 0..3 {
            let mut answer = self.call_actor(&region, ask).await;
            if let Some(saved) = answer.durable.take()
                && let Err(e) = saved.await
            {
                return Err(ApiError::internal(format!("could not save: {e}")));
            }
            match answer.next.take() {
                Some(Next::Again(job, call)) => {
                    let made = offload::run_job(job)
                        .await
                        .ok_or_else(keys::no_randomness)?;
                    ask = Ask::Again(call, made);
                }
                Some(Next::Answer(deferred)) => {
                    let reply = offload::run_deferred(deferred.run).await;
                    if reply.is_ok()
                        && let Some((id, used)) = deferred.used
                    {
                        drop(self.call_actor(&region, Ask::Used(id, used)).await);
                    }
                    return reply;
                }
                None => {
                    return match answer.fanout.take() {
                        Some(fanout) => self.spread(fanout, answer.reply).await,
                        None => answer.reply,
                    };
                }
            }
        }
        Err(ApiError::internal(
            "the key material could not be made: the key kept changing",
        ))
    }

    /// Sends one region's actor work from another, and waits for it to be on disk.
    async fn send(&self, region: &str, ask: crate::core::Ask) -> Reply {
        let actor = self
            .actors
            .actor(region)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let mut answer = actor
            .call(ask)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(saved) = answer.durable.take() {
            saved
                .await
                .map_err(|e| ApiError::internal(format!("could not save: {e}")))?;
        }
        answer.reply
    }

    /// Does the work a multi-Region call leaves for other regions, outside every actor (no actor waits on another): the replica
    /// first (its answer is the call's), then the set's new shape in each region of it, then new material for the replicas.
    async fn spread(&self, fanout: multi::Fanout, mut reply: Reply) -> Reply {
        use crate::core::Ask;
        if let Some((region, seed)) = fanout.create {
            reply = Ok(self.send(&region, Ask::Replica(seed)).await?);
        }
        if let Some((id, set, regions)) = fanout.set {
            for region in regions {
                // WHY: a region that cannot take the new shape reports it on its own next call; the call itself was done.
                drop(self.send(&region, Ask::Set(id.clone(), set.clone())).await);
            }
        }
        if let Some((id, materials, regions)) = fanout.materials {
            for region in regions {
                drop(
                    self.send(&region, Ask::Materials(id.clone(), materials.clone()))
                        .await,
                );
            }
        }
        reply
    }
}

impl Service for Kms {
    fn storage(&self) -> Option<&dyn aws_wire::storage::Storage> {
        Some(self)
    }

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

    /// The Coral front end's wording, with the grant operations listed in AWS's order rather than the reverse of the model's.
    fn invalid(&self, violation: &aws_schema::json::Violation) -> aws_wire::ApiError {
        let error = aws_wire::service::default_invalid(&TABLES, violation);
        match violation {
            aws_schema::json::Violation::Enum { member, .. } if is_grant_operation(*member) => {
                aws_wire::located::enum_order(error, &grants::OPERATIONS)
            }
            _ => error,
        }
    }
}

/// Whether the model member `ix` holds a grant operation: its enum set is exactly those.
fn is_grant_operation(ix: u32) -> bool {
    TABLES
        .member(ix)
        .and_then(|m| TABLES.cons(m.cons))
        .map(|c| TABLES.enum_values(c.enum_set))
        .is_some_and(|set| {
            set.len() == grants::OPERATIONS.len()
                && set.iter().all(|v| grants::OPERATIONS.contains(v))
        })
}

/// Whether `text` is a key policy this service accepts. Exists so the check can be fuzzed.
#[doc(hidden)]
#[must_use]
pub fn check_key_policy(text: &str) -> bool {
    policy::is_policy(text)
}

#[cfg(test)]
mod tests;
