//! The SASL flags: `--sasl-mechanisms`, `--sasl-user`, `--sasl-users-file`, the variable `DOZE_KAFKA_SASL_USERS` and
//! `--sasl-allow-empty-users`, checked in the Go edition's words. The users from these are the **bootstrap** users:
//! derived at every start and never stored (the users made over the API are the stored ones).

use std::convert::Infallible;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use kafka_auth::{Authenticator, Credentials, Mechanism, Users};

mod source;
pub(crate) use source::permission_warning;

/// A `--sasl-user` value as typed, `user:password`. Its `Debug` shows nothing: it holds a password.
#[derive(Clone)]
pub(super) struct Login(String);

impl FromStr for Login {
    type Err = Infallible;

    /// Any text: whether it is a login is judged later, in words that leave out what was typed (a parser
    /// error of clap would repeat the value, password and all).
    fn from_str(s: &str) -> Result<Self, Infallible> {
        Ok(Self(s.to_owned()))
    }
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Login(<redacted>)")
    }
}

/// Everything the SASL flags and the environment said.
pub(super) struct Input<'a> {
    pub(super) mechanisms: &'a [String],
    pub(super) users: &'a [Login],
    pub(super) users_file: Option<&'a Path>,
    /// The value of `DOZE_KAFKA_SASL_USERS`, if it is set and not empty.
    pub(super) env: Option<String>,
    pub(super) allow_empty: bool,
    pub(super) data_dir: &'a Path,
}

/// What they come to.
pub(super) struct Sasl {
    /// Set if anybody is to be asked to log in.
    pub(super) authenticator: Option<Arc<Authenticator>>,
    /// The users, shared with the broker, which loads the stored ones into it.
    pub(super) users: Arc<Users>,
}

/// Who may log in: nobody is asked to unless `--sasl-mechanisms` is given, and then somebody must be able to, now or
/// once the stored users are loaded. The checks and their words are the Go edition's; none repeats what was typed
/// after a colon.
pub(super) fn setup(input: &Input<'_>) -> Result<Sasl> {
    let configured = !input.users.is_empty() || input.users_file.is_some() || input.env.is_some();
    if input.mechanisms.is_empty() {
        if configured {
            bail!(
                "SASL users are configured but no mechanism is enabled; add --sasl-mechanisms (PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512), or drop the users"
            );
        }
        if input.allow_empty {
            bail!(
                "--sasl-allow-empty-users needs --sasl-mechanisms; without SASL nobody is asked to log in"
            );
        }
        return Ok(Sasl {
            authenticator: None,
            users: Arc::new(Users::none()),
        });
    }
    let mechanisms = input
        .mechanisms
        .iter()
        .map(|name| {
            Mechanism::from_name(name).ok_or_else(|| {
                anyhow!(
                    "unknown SASL mechanism {name:?}; supported: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512"
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let pairs = pairs(input)?;
    if pairs.is_empty() && !input.allow_empty && !may_hold_users(input.data_dir) {
        bail!("{NO_USERS}");
    }
    let users = Arc::new(Users::derive(Credentials::new(pairs)?, &mechanisms)?);
    let authenticator = Authenticator::over(&mechanisms, Arc::clone(&users))?;
    Ok(Sasl {
        authenticator: Some(Arc::new(authenticator)),
        users,
    })
}

/// The words of the refusal to start with SASL on and nobody to log in; the broker says them too, if it finds no stored
/// user when it opens its database.
pub(crate) const NO_USERS: &str = "SASL is enabled but no credentials are configured; give --sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS, or start with --sasl-allow-empty-users to make the first user over the API (as a super user)";

/// Whether the data directory has a database, which may hold stored users. Without one it cannot, and the start is refused
/// before anything is created; with one, whether it does is known only once the broker has opened it.
fn may_hold_users(data_dir: &Path) -> bool {
    data_dir.join("meta.redb").exists()
}

/// The users of the three sources, in the order flag, file, environment.
fn pairs(input: &Input<'_>) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for Login(text) in input.users {
        let (user, password) = text
            .split_once(':')
            .ok_or_else(|| anyhow!("--sasl-user: not a valid value (expected user:password)"))?;
        out.push((user.to_owned(), password.to_owned()));
    }
    if let Some(path) = input.users_file {
        let text = source::read_file(path).map_err(|e| anyhow!(e))?;
        if let Some(warning) = permission_warning(path) {
            tracing::warn!("{warning}");
        }
        out.extend(source::parse_file(&text).map_err(|e| anyhow!(e))?);
    }
    if let Some(text) = &input.env {
        out.extend(source::parse_env(text).map_err(|e| anyhow!(e))?);
    }
    Ok(out)
}
