//! Cryptography too slow for the event loop, done on a blocking thread. KMS runs on the server's one event loop, and some of its work
//! takes milliseconds to seconds: making an RSA key (half a second to two seconds at 4,096 bits, depending on how soon a prime turns
//! up), an RSA private operation (about 12 ms at 4,096 bits in the size-built release), an elliptic-curve signature or check (up to
//! about 2 ms for P-384). Every other call of every service waits while the loop is held.
//!
//! Two shapes of work leave the actor:
//! - **Made, then asked again.** A call that needs something new (a key's first RSA material, a data key pair, a wrapping key, the
//!   material an import unwrapped) checks what it can, then hands back a [`Job`] and the call. The job runs on a blocking thread and
//!   the call is asked again with what it made ([`Made`]), which the actor takes in place of making it. The second asking checks
//!   everything again: what changed in between is seen.
//! - **Answered off the loop.** A call whose last step is the slow one (a signature, a check, an RSA decryption) checks everything
//!   and hands back a [`Deferred`]: a closure over the key material and the request that makes the answer. A successful one is then
//!   noted as the key's last use.
//!
//! Only the requested work is done, once: nothing is made ahead of time, so an idle KMS costs nothing.

use std::fmt;

use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::agree::Used;
use crate::core::Core;
use crate::rsa_keys;
use crate::spec::Spec;

/// What a [`Job`] made, which the call asked again takes.
#[derive(Debug)]
pub(crate) enum Made {
    /// A new RSA key: its size, the PKCS #8 private key and the DER `SubjectPublicKeyInfo`.
    Rsa {
        bits: usize,
        der: Vec<u8>,
        public: Vec<u8>,
    },
    /// A data key pair of a spec: the PKCS #8 private key and the DER `SubjectPublicKeyInfo`.
    Pair {
        spec: Spec,
        private: Vec<u8>,
        public: Vec<u8>,
    },
    /// The bytes an import unwrapped (`None`: the wrapping did not open).
    Unwrapped(Option<Vec<u8>>),
}

/// Work to run off the loop before the call is asked again.
pub(crate) type Job = Box<dyn FnOnce() -> Option<Made> + Send>;

/// An answer made off the loop, and the use it is noted as when it succeeds.
pub(crate) struct Deferred {
    pub(crate) run: Box<dyn FnOnce() -> Reply + Send>,
    pub(crate) used: Option<(String, Used)>,
}

impl fmt::Debug for Deferred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deferred")
            .field("used", &self.used)
            .finish_non_exhaustive()
    }
}

/// What the call leaves to be done off the loop.
pub(crate) enum Next {
    /// Run the job, then ask the call again with what it made.
    Again(Job, Box<Call>),
    /// Run the closure: its reply is the call's.
    Answer(Deferred),
}

impl fmt::Debug for Next {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Again(_, call) => write!(f, "Again({})", call.id),
            Self::Answer(d) => d.fmt(f),
        }
    }
}

/// What the call in hand leaves: a job to run before it is asked again, or its answer to make.
pub(crate) enum Pending {
    Job(Job),
    Answer(Deferred),
}

impl Pending {
    /// What the actor hands back for it.
    pub(crate) fn next(self, call: Call) -> Next {
        match self {
            Self::Job(job) => Next::Again(job, Box::new(call)),
            Self::Answer(d) => Next::Answer(d),
        }
    }
}

/// The state of the call in hand: what it was given, and what it leaves.
#[derive(Default)]
pub(crate) struct Offload {
    /// What a job made for this call.
    pub(crate) made: Option<Made>,
    /// The job the call needs run before it can go on.
    pub(crate) job: Option<Job>,
    /// The answer the call leaves to be made.
    pub(crate) deferred: Option<Deferred>,
}

impl fmt::Debug for Offload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Offload")
            .field("made", &self.made.is_some())
            .field("job", &self.job.is_some())
            .field("deferred", &self.deferred)
            .finish()
    }
}

/// The error a call returns while it waits for its job: never seen by a caller, the call is asked again.
fn waiting() -> ApiError {
    ApiError::internal("the call is waiting for work off the event loop")
}

/// A new RSA key of `bits` with its public half.
pub(crate) fn make_rsa(bits: usize) -> Option<Made> {
    let der = rsa_keys::generate(bits)?;
    let public = rsa_keys::public_key(&der)?;
    Some(Made::Rsa { bits, der, public })
}

/// Runs `job` on a blocking thread.
pub(crate) async fn run_job(job: Job) -> Option<Made> {
    tokio::task::spawn_blocking(job).await.ok().flatten()
}

/// Makes the deferred answer on a blocking thread.
pub(crate) async fn run_deferred(run: Box<dyn FnOnce() -> Reply + Send>) -> Reply {
    tokio::task::spawn_blocking(run)
        .await
        .unwrap_or_else(|e| Err(ApiError::internal(format!("the answer was not made: {e}"))))
}

impl Core {
    /// A new RSA key of `bits` (PKCS #8 and public key): the one made for this call, or, when there is none, a request to make one
    /// (the call stops here and is asked again).
    pub(crate) fn fresh_rsa(&mut self, bits: usize) -> Result<(Vec<u8>, Vec<u8>), ApiError> {
        match self.offload.made.take() {
            Some(Made::Rsa {
                bits: b,
                der,
                public,
            }) if b == bits => Ok((der, public)),
            _ => {
                self.offload.job = Some(Box::new(move || make_rsa(bits)));
                Err(waiting())
            }
        }
    }

    /// A new key pair of `spec` (an RSA or elliptic-curve one) as a data key pair hands it out, made as [`fresh_rsa`](Self::fresh_rsa) is.
    pub(crate) fn fresh_pair(&mut self, spec: Spec) -> Result<(Vec<u8>, Vec<u8>), ApiError> {
        match self.offload.made.take() {
            Some(Made::Pair {
                spec: s,
                private,
                public,
            }) if s == spec => Ok((private, public)),
            _ => {
                self.offload.job = Some(Box::new(move || {
                    let secret = crate::asym::generate(spec)?;
                    Some(Made::Pair {
                        spec,
                        private: crate::asym::private_key(spec, &secret)?,
                        public: crate::asym::public_key(spec, &secret)?,
                    })
                }));
                Err(waiting())
            }
        }
    }

    /// What `unwrap` opens: the bytes made for this call, or, when there are none, a request to open them off the loop.
    pub(crate) fn unwrapped(
        &mut self,
        unwrap: impl FnOnce() -> Option<Vec<u8>> + Send + 'static,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        if let Some(Made::Unwrapped(raw)) = self.offload.made.take() {
            return Ok(raw);
        }
        self.offload.job = Some(Box::new(move || Some(Made::Unwrapped(unwrap()))));
        Err(waiting())
    }

    /// Leaves the answer of `call` to `run`, off the loop. The reply returned now stands in for it.
    pub(crate) fn defer(
        &mut self,
        call: &Call,
        run: impl FnOnce() -> Reply + Send + 'static,
    ) -> Reply {
        self.offload.deferred = Some(Deferred {
            run: Box::new(run),
            used: self.use_of(call),
        });
        aws_wire::handler::empty()
    }

    /// What the call just dispatched leaves (a job it asked for, or the answer it deferred when it succeeded so far), and forgets
    /// what was made for it.
    pub(crate) fn pending(&mut self, ok: bool) -> Option<Pending> {
        self.offload.made = None;
        let deferred = self.offload.deferred.take();
        if let Some(job) = self.offload.job.take() {
            return Some(Pending::Job(job));
        }
        deferred.filter(|_| ok).map(Pending::Answer)
    }

    /// Runs a call as the actor and the service between them do, with the work done inline: for tests of the rules.
    #[cfg(test)]
    pub(crate) fn dispatch_inline(&mut self, call: &Call) -> Reply {
        for _ in 0..3 {
            let reply = self.dispatch(call);
            match self.pending(reply.is_ok()) {
                Some(Pending::Job(job)) => {
                    self.offload.made = Some(job().ok_or_else(crate::keys::no_randomness)?)
                }
                Some(Pending::Answer(d)) => {
                    let reply = (d.run)();
                    if reply.is_ok()
                        && let Some((id, used)) = d.used
                    {
                        self.used.insert(id, used);
                    }
                    return reply;
                }
                None => {
                    if reply.is_ok() {
                        self.note_use(call);
                    }
                    return reply;
                }
            }
        }
        Err(crate::keys::no_randomness())
    }
}

impl crate::core::Answer {
    /// A reply with nothing to wait for and nothing left to do.
    pub(crate) fn of(reply: Reply) -> Self {
        Self {
            reply,
            durable: None,
            fanout: None,
            next: None,
        }
    }
}
