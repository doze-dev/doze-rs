//! A SCRAM client, for tests only (the `client` feature, and this crate's own tests).
//!
//! It is written from RFC 5802 alone and shares nothing with the server side but the hash and HMAC functions, so
//! a test that logs this client in is a check of the server and not of itself. It builds the messages the way
//! the Go conformance harness does (`scenarios_sasl.go`), and lets a test change any part of them.

#![allow(clippy::expect_used)] // a test client does not recover from a broken server

use std::num::NonZeroU32;

use super::Kind;
use crate::crypto::base64;

/// One client login in progress.
#[derive(Debug, Clone)]
pub struct Client {
    kind: Kind,
    password: Vec<u8>,
    gs2: String,
    bare: String,
}

/// The client-final message, and the server-final message a correct server answers it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Final {
    /// What to send.
    pub message: Vec<u8>,
    /// `v=<ServerSignature>`, which the client checks the server's answer against.
    pub expected: Vec<u8>,
}

impl Client {
    /// A client for `user` with `password`, using `nonce`. The header is `n,,`.
    #[must_use]
    pub fn new(kind: Kind, user: &str, password: &str, nonce: &str) -> Self {
        Self {
            kind,
            password: password.as_bytes().to_vec(),
            gs2: "n,,".to_owned(),
            bare: format!(
                "n={},r={nonce}",
                user.replace('=', "=3D").replace(',', "=2C")
            ),
        }
    }

    /// The same client with another gs2 header (`n,a=alice,`) in front of the bare message.
    #[must_use]
    pub fn with_header(mut self, gs2: &str) -> Self {
        gs2.clone_into(&mut self.gs2);
        self
    }

    /// The same client with `prefix` (such as `m=ext,`) in front of the user name, inside the message it signs,
    /// as the conformance harness does for a reserved extension.
    #[must_use]
    pub fn with_signed_prefix(mut self, prefix: &str) -> Self {
        self.bare.insert_str(0, prefix);
        self
    }

    /// The same client with `extra` fields after the nonce (`,x=ext`), which it signs.
    #[must_use]
    pub fn with_extensions(mut self, extra: &str) -> Self {
        self.bare.push_str(extra);
        self
    }

    /// The client-first message.
    #[must_use]
    pub fn first(&self) -> Vec<u8> {
        format!("{}{}", self.gs2, self.bare).into_bytes()
    }

    /// The client-final message answering `server_first`, with a correct proof, the channel binding
    /// `c=<binding>` (base64 of the gs2 header if `None`) and `extra` (such as `,x=ext`) before the proof.
    ///
    /// # Panics
    /// If `server_first` is not `r=...,s=<base64>,i=<n>`: a test client does not recover from a broken server.
    #[must_use]
    pub fn last(&self, server_first: &[u8], binding: Option<&str>, extra: &str) -> Final {
        let text = std::str::from_utf8(server_first).expect("server-first is text");
        let mut fields = text.split(',');
        let nonce = fields
            .next()
            .and_then(|f| f.strip_prefix("r="))
            .expect("r=");
        let salt = fields
            .next()
            .and_then(|f| f.strip_prefix("s="))
            .expect("s=");
        let iterations = fields
            .next()
            .and_then(|f| f.strip_prefix("i="))
            .expect("i=");
        let salt = base64::decode(salt.as_bytes()).expect("salt is base64");
        let iterations: NonZeroU32 = iterations.parse().expect("an iteration count");

        let channel = binding.map_or_else(|| base64::encode(self.gs2.as_bytes()), str::to_owned);
        let without_proof = format!("c={channel},r={nonce}{extra}");
        let auth_message = format!("{},{text},{without_proof}", self.bare);

        let salted = self.kind.salted_password(&self.password, &salt, iterations);
        let client_key = self.kind.hmac(&salted, &[b"Client Key"]);
        let stored_key = self.kind.hash(&client_key);
        let signature = self.kind.hmac(&stored_key, &[auth_message.as_bytes()]);
        let proof: Vec<u8> = client_key
            .iter()
            .zip(&signature)
            .map(|(k, s)| k ^ s)
            .collect();
        let server_key = self.kind.hmac(&salted, &[b"Server Key"]);
        let server_signature = self.kind.hmac(&server_key, &[auth_message.as_bytes()]);
        Final {
            message: format!("{without_proof},p={}", base64::encode(&proof)).into_bytes(),
            expected: format!("v={}", base64::encode(&server_signature)).into_bytes(),
        }
    }
}
