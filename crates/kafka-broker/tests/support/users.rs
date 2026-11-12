//! Requests for the two SCRAM credential APIs, and the broker's users to look at.
#![allow(
    unreachable_pub,
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;

use super::Rig;
use super::acls::user;
use kafka_auth::scram::client::Client;
use kafka_auth::scram::{self, Kind};
use kafka_auth::{Credentials, Mechanism, Users};
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_user_scram_credentials_request::{
    ScramCredentialDeletion, ScramCredentialUpsertion,
};
use kafka_wire::messages::describe_user_scram_credentials_request::UserName;
use kafka_wire::messages::{
    AlterUserScramCredentialsRequest, DescribeUserScramCredentialsRequest,
    DescribeUserScramCredentialsResponse,
};

pub const SHA256: i8 = 1;
pub const SHA512: i8 = 2;

fn text(s: &str) -> StrBytes {
    StrBytes::from_string(s.to_owned())
}

pub fn kind_of(mechanism: i8) -> Kind {
    if mechanism == SHA256 {
        Kind::Sha256
    } else {
        Kind::Sha512
    }
}

/// What a client sends for a password: `Hi(password, salt, iterations)`.
pub fn salted(mechanism: i8, password: &str, salt: &[u8], iterations: i32) -> Vec<u8> {
    kind_of(mechanism).salted_password(
        password.as_bytes(),
        salt,
        std::num::NonZeroU32::new(iterations.max(1).unsigned_abs()).unwrap(),
    )
}

/// An upsertion as `kafka-configs.sh` makes it: the salt is the client's, the salted password is computed there.
pub fn upsert(
    user: &str,
    mechanism: i8,
    iterations: i32,
    password: &str,
) -> ScramCredentialUpsertion {
    let salt = format!("salt-of-{user}").into_bytes();
    let mut u = ScramCredentialUpsertion::default();
    u.name = text(user);
    u.mechanism = mechanism;
    u.iterations = iterations;
    // The broker does not check that the count was the one used (it cannot), so an absurd count costs nothing to ask.
    let worked = iterations.clamp(1, 20_000);
    u.salted_password = salted(mechanism, password, &salt, worked).into();
    u.salt = salt.into();
    u
}

pub fn delete(user: &str, mechanism: i8) -> ScramCredentialDeletion {
    let mut d = ScramCredentialDeletion::default();
    d.name = text(user);
    d.mechanism = mechanism;
    d
}

/// One line of an Alter answer: the user, the code and the message.
pub type Line = (String, i16, Option<String>);

/// Users configured at start (name, password), with a SCRAM verifier for both mechanisms. Give a clone to the broker's
/// `Config::users` and keep one to try a login with.
pub fn bootstrap_users(bootstrap: &[(&str, &str)]) -> Arc<Users> {
    let pairs = bootstrap
        .iter()
        .map(|(u, p)| ((*u).to_owned(), (*p).to_owned()));
    let creds = Credentials::new(pairs).unwrap();
    let mechanisms = [Mechanism::ScramSha256, Mechanism::ScramSha512];
    Arc::new(Users::derive(creds, &mechanisms).unwrap())
}

/// Whether `user` with `password` completes a SCRAM exchange against `users` for `mechanism`.
pub fn logs_in(users: &Users, mechanism: i8, user: &str, password: &str) -> bool {
    let kind = kind_of(mechanism);
    let Some(v) = users.verifier(kind, user) else {
        return false;
    };
    let client = Client::new(kind, user, password, "nonce.");
    let Ok((server_first, awaiting)) = scram::first(kind, &client.first(), "SN", |_| Some(v))
    else {
        return false;
    };
    awaiting
        .finish(&client.last(&server_first, None, "").message)
        .is_ok()
}

impl Rig {
    pub async fn scram_alter_as(
        &self,
        who: &Caller,
        deletions: Vec<ScramCredentialDeletion>,
        upsertions: Vec<ScramCredentialUpsertion>,
    ) -> Vec<Line> {
        let mut req = AlterUserScramCredentialsRequest::default();
        req.deletions = deletions;
        req.upsertions = upsertions;
        let Response::AlterUserScramCredentials(r) = self
            .ask_as(who, 0, Request::AlterUserScramCredentials(req))
            .await
        else {
            panic!("not AlterUserScramCredentials")
        };
        r.results
            .into_iter()
            .map(|x| {
                (
                    x.user.to_string(),
                    x.error_code,
                    x.error_message.map(|m| m.to_string()),
                )
            })
            .collect()
    }

    /// Alter as the super user of `closed_world`... or anybody, on a broker that is open.
    pub async fn scram_alter(
        &self,
        deletions: Vec<ScramCredentialDeletion>,
        upsertions: Vec<ScramCredentialUpsertion>,
    ) -> Vec<Line> {
        self.scram_alter_as(&user("admin"), deletions, upsertions)
            .await
    }

    pub async fn scram_describe_as(
        &self,
        who: &Caller,
        users: Option<&[&str]>,
    ) -> DescribeUserScramCredentialsResponse {
        let mut req = DescribeUserScramCredentialsRequest::default();
        req.users = users.map(|names| {
            names
                .iter()
                .map(|n| {
                    let mut u = UserName::default();
                    u.name = text(n);
                    u
                })
                .collect()
        });
        let Response::DescribeUserScramCredentials(r) = self
            .ask_as(who, 0, Request::DescribeUserScramCredentials(req))
            .await
        else {
            panic!("not DescribeUserScramCredentials")
        };
        r
    }

    /// What Describe lists for everybody: (user, [(mechanism, iterations)]), sorted.
    pub async fn scram_listed(&self) -> Vec<(String, Vec<(i8, i32)>)> {
        let r = self.scram_describe_as(&user("admin"), None).await;
        assert_eq!(
            (r.error_code, r.error_message.clone()),
            (0, Some(StrBytes::from_static_str("")))
        );
        let mut out: Vec<_> = r
            .results
            .iter()
            .map(|x| {
                assert_eq!(
                    (x.error_code, x.error_message.as_deref()),
                    (0, Some("")),
                    "{x:?}"
                );
                (
                    x.user.to_string(),
                    x.credential_infos
                        .iter()
                        .map(|i| (i.mechanism, i.iterations))
                        .collect(),
                )
            })
            .collect();
        out.sort();
        out
    }
}
