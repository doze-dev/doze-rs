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

/// What happens to a request IAM would refuse.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum IamMode {
    /// Policies are stored and never asked.
    Off,
    /// A request that would be refused is logged, and goes through.
    Soft,
    /// A request that is not allowed is refused, in AWS's words.
    Enforce,
}

impl From<IamMode> for aws_peers::Mode {
    fn from(m: IamMode) -> Self {
        match m {
            IamMode::Off => Self::Off,
            IamMode::Soft => Self::Soft,
            IamMode::Enforce => Self::Enforce,
        }
    }
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
    /// What IAM does with a request its policies do not allow. Callers using a key IAM did not issue (`test`) are the account
    /// root and are allowed everything.
    #[arg(long, value_enum, default_value_t = IamMode::Soft)]
    pub(crate) iam: IamMode,
    /// Seconds without a request before a service lets go of its memory and its files (it starts again, with its state, on the
    /// next request).
    #[arg(long, default_value_t = 300)]
    pub(crate) idle_secs: u64,
    /// The account every request is for.
    #[arg(long, default_value = "000000000000")]
    pub(crate) account: String,
    /// Seconds a warm Lambda function process waits for its next invocation before it ends.
    #[arg(long, default_value_t = 600)]
    pub(crate) lambda_idle_secs: u64,
    /// Keep what Lambda functions print out of this process's log (it is always in CloudWatch Logs).
    #[arg(long)]
    pub(crate) lambda_quiet: bool,
}
