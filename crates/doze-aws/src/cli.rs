//! The command line.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// How requests are authenticated.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum AuthMode {
    /// Route by the credential scope and accept any access key (for local development).
    Trust,
    /// Check every signature against `--access-key` and `--secret-key`.
    Verify,
}

/// Local AWS for development.
#[derive(Debug, Parser)]
#[command(version, about)]
pub(crate) struct Args {
    /// Where to listen.
    #[arg(long, default_value = "127.0.0.1:4566")]
    pub(crate) listen: SocketAddr,
    /// Where state is kept.
    #[arg(long, default_value = "./doze-aws-data")]
    pub(crate) data_dir: PathBuf,
    /// How requests are authenticated.
    #[arg(long, value_enum, default_value_t = AuthMode::Trust)]
    pub(crate) auth: AuthMode,
    /// The access key `--auth verify` accepts.
    #[arg(long, default_value = "test")]
    pub(crate) access_key: String,
    /// The secret key `--auth verify` accepts.
    #[arg(long, default_value = "test")]
    pub(crate) secret_key: String,
    /// Seconds without a request before a service lets go of its memory and its files (it starts again, with its state, on the
    /// next request).
    #[arg(long, default_value_t = 300)]
    pub(crate) idle_secs: u64,
    /// The account every request is for.
    #[arg(long, default_value = "000000000000")]
    pub(crate) account: String,
}
