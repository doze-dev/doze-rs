//! Test support for the AWS services: a request as JSON is decoded against the model like the gateway would, and an answer is
//! read back as JSON. Only tests depend on it.
#![allow(clippy::unwrap_used, clippy::panic)]

use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_schema::tables::Tables;
use aws_wire::service::{Call, Caller, Output, Service};
use aws_wire::{ApiError, RequestId};
use serde_json::{Value, json};

/// A model, and the service that words its failed checks.
#[derive(Debug)]
pub struct Harness<S: Service> {
    service: S,
    patterns: Patterns,
    account: String,
    region: String,
    /// The session token the calls carry, when a test sets one.
    pub session_token: Option<String>,
    /// The access key id the calls carry.
    pub access_key_id: String,
}

impl<S: Service> Harness<S> {
    /// A harness for `service`, calling as account `000000000000` in `us-east-1`.
    pub fn new(service: S) -> Self {
        let patterns = Patterns::new(service.tables());
        Self {
            service,
            patterns,
            account: "000000000000".into(),
            region: "us-east-1".into(),
            session_token: None,
            access_key_id: "test".into(),
        }
    }

    fn tables(&self) -> &'static Tables {
        self.service.tables()
    }

    /// The operation called `name`.
    pub fn op(&self, name: &str) -> u32 {
        self.tables()
            .find_op(name)
            .unwrap_or_else(|| panic!("no operation {name}"))
    }

    /// Whether the service runs `name` (and does not refuse it by name).
    pub fn serves(&self, name: &str) -> bool {
        self.service.serves(self.op(name))
    }

    /// `request` decoded against operation `op`, as the gateway would hand it over.
    ///
    /// # Errors
    /// The service's wording of a failed check.
    pub fn call(&self, op: u32, request: &Value) -> Result<Call, ApiError> {
        let body = request.to_string();
        let mut arena = Arena::new();
        // An operation with no input takes no body.
        let root = match self.tables().op(op).unwrap().input {
            Some(shape) => decode(self.tables(), &self.patterns, shape, &body, &mut arena)
                .map_err(|v| self.service.invalid_in(&v, &body))?,
            None => NIL,
        };
        Ok(Call {
            op,
            id: RequestId::new(),
            caller: Caller {
                access_key_id: self.access_key_id.clone(),
                account_id: self.account.clone(),
                region: self.region.clone(),
                endpoint: "http://localhost:4566".into(),
                service: None,
                session_token: self.session_token.clone(),
            },
            body,
            arena,
            root,
        })
    }

    /// An answer as JSON.
    pub fn json(&self, op: u32, out: &Output) -> Value {
        let Some(shape) = self.tables().op(op).unwrap().output else {
            return json!({});
        };
        if out.root == NIL {
            return json!({});
        }
        let mut bytes = Vec::new();
        encode(self.tables(), shape, &out.arena, "", out.root, &mut bytes).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}

/// A directory of its own that goes away with it.
#[derive(Debug)]
pub struct TempDir(std::path::PathBuf);

impl TempDir {
    /// A new empty directory.
    pub fn new(tag: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("doze-aws-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// Where it is.
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}
