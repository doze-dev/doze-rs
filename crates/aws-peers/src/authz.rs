//! Authorization, as the services see it: before a request does anything, the service says what it asks to do (`sqs:SendMessage` on a
//! queue's ARN) and whatever policy the resource itself carries, and the authorizer says yes or refuses in AWS's words. The
//! authorizer is IAM; services know nothing of principals, sessions or policy documents.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Caller};

use crate::Peers;

/// One thing a request asks to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Need {
    /// `sqs:SendMessage`.
    pub action: String,
    /// The ARN acted on; `*` for an action that is on no resource.
    pub resource: String,
    /// Context keys the request itself supplies beyond the caller's own (`kms:ViaService`, `ssm:resourceTag/team`, `aws:RequestTag/x`).
    pub context: Vec<(String, Vec<String>)>,
    /// The policy document on the resource (a queue's `Policy`, a secret's resource policy, a key's policy), when it has one.
    pub policy: Option<String>,
    /// The resource must allow the caller or its account in its own policy before identity policies count (a KMS key).
    pub gate: bool,
}

impl Need {
    /// A need for `action` on `resource` with nothing else to say.
    #[must_use]
    pub fn new(action: impl Into<String>, resource: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            resource: resource.into(),
            ..Self::default()
        }
    }

    /// With a key the request supplies.
    #[must_use]
    pub fn with(mut self, key: &str, values: Vec<String>) -> Self {
        self.context.push((key.to_owned(), values));
        self
    }

    /// With the policy on the resource.
    #[must_use]
    pub fn with_policy(mut self, policy: Option<String>) -> Self {
        self.policy = policy;
        self
    }

    /// With the gate set.
    #[must_use]
    pub const fn gated(mut self) -> Self {
        self.gate = true;
        self
    }
}

/// What the authorizer does with a refusal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Nothing is asked.
    Off,
    /// A request that would be refused is logged, and goes through.
    #[default]
    Soft,
    /// A request that is not allowed is refused.
    Enforce,
}

impl Mode {
    const fn byte(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Soft => 1,
            Self::Enforce => 2,
        }
    }

    const fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Off,
            2 => Self::Enforce,
            _ => Self::Soft,
        }
    }
}

/// The mode, shared by the bus and read by whoever decides.
#[derive(Debug, Default)]
pub(crate) struct ModeCell(AtomicU8);

/// Decides whether a caller may do what a request asks. Implemented by IAM.
pub trait Authorizer: Send + Sync + 'static {
    /// Every need must be met. The error is the refusal the caller sees.
    fn authorize<'a>(
        &'a self,
        caller: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>>;
}

impl Peers {
    /// Sets what a refusal does.
    pub fn set_mode(&self, mode: Mode) {
        self.mode.0.store(mode.byte(), Ordering::Relaxed);
    }

    /// What a refusal does.
    #[must_use]
    pub fn mode(&self) -> Mode {
        Mode::from_byte(self.mode.0.load(Ordering::Relaxed))
    }

    /// Installs the authorizer. Until there is one, everything is allowed.
    pub fn set_authorizer(&self, authorizer: Arc<dyn Authorizer>) {
        if let Ok(mut slot) = self.authorizer.write() {
            *slot = Some(authorizer);
        }
    }

    /// Asks whether `caller` may do all of `needs`. A call made by a service for a caller (SSM sealing with KMS) was authorized when it
    /// came in over the wire and is not asked again.
    ///
    /// # Errors
    /// The authorizer's refusal.
    pub async fn authorize(&self, caller: &Caller, needs: Vec<Need>) -> Result<(), ApiError> {
        if caller.service.is_some() || needs.is_empty() || self.mode() == Mode::Off {
            return Ok(());
        }
        let authorizer = self.authorizer.read().ok().and_then(|slot| slot.clone());
        match authorizer {
            Some(a) => a.authorize(caller, needs).await,
            None => Ok(()),
        }
    }
}
