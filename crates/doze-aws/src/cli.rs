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
    /// Days a log event is kept locally, whatever its group's retention (a bound on the local store, not AWS behaviour: AWS keeps
    /// events forever by default, and groups still report no retention). 0 keeps them.
    #[arg(long, default_value_t = 7)]
    pub(crate) logs_retention_days: u64,
    /// The most log events one group keeps locally; past it the oldest go. 0 is no cap.
    #[arg(long, default_value_t = 100_000)]
    pub(crate) logs_max_events: u64,
    /// Hours CloudWatch metric data is kept (AWS keeps one-minute data for 15 days).
    #[arg(long, default_value_t = 24, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) cloudwatch_retention_hours: u64,
    /// What CloudFormation does with resource types doze-aws does not make: each type's own class (opaque infrastructure is skipped
    /// with a warning on the resource, behaviour-bearing types are refused), or skip all of them, or refuse all of them.
    #[arg(long, value_enum, default_value_t = CfnUnsupported::Default)]
    pub(crate) cfn_unsupported: CfnUnsupported,
    /// A resource type CloudFormation accepts unmade whatever its class (repeatable).
    #[arg(long = "cfn-skip", value_name = "TYPE")]
    pub(crate) cfn_skip: Vec<String>,
    /// A resource type CloudFormation refuses whatever its class (repeatable).
    #[arg(long = "cfn-fail", value_name = "TYPE")]
    pub(crate) cfn_fail: Vec<String>,
    /// Name resources a template leaves unnamed after their logical id, instead of AWS's `<stack>-<logical>-<random>`.
    #[arg(long)]
    pub(crate) cfn_logical_names: bool,
    /// Let a deleted SQS queue's name be used again at once, and a queue be purged again at once. AWS makes both wait 60 seconds
    /// (`QueueDeletedRecently`, `PurgeQueueInProgress`), and so does doze-aws by default; this is for local test loops that make and
    /// drop the same queue over and over.
    #[arg(long)]
    pub(crate) sqs_skip_60s_waits: bool,
    /// Milliseconds a new DynamoDB table (or index) stays `CREATING` before it is `ACTIVE`. AWS takes seconds to minutes; the default,
    /// 0, lets a waiter pass on its first probe. Raise it to test code that waits. Also read from `DOZE_DDB_CREATE_MS`.
    #[arg(long, value_name = "MS")]
    pub(crate) dynamodb_create_ms: Option<u64>,
    /// Milliseconds a DynamoDB table stays `UPDATING` after `UpdateTable` (default 0). Also read from `DOZE_DDB_UPDATE_MS`.
    #[arg(long, value_name = "MS")]
    pub(crate) dynamodb_update_ms: Option<u64>,
    /// Milliseconds a DynamoDB table stays `DELETING` after `DeleteTable` (default 0). Also read from `DOZE_DDB_DELETE_MS`.
    #[arg(long, value_name = "MS")]
    pub(crate) dynamodb_delete_ms: Option<u64>,
}

/// What CloudFormation does with types it does not make.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum CfnUnsupported {
    /// Each type's own class.
    Default,
    /// Skip every unmade type, with a warning on the resource.
    Skip,
    /// Refuse every unmade type.
    Fail,
}

impl Args {
    /// DynamoDB's transitional windows: each flag, else its environment variable (the older spelling), else 0.
    pub(crate) fn dynamodb_windows(&self) -> aws_dynamodb::Windows {
        let pick = |flag: Option<u64>, env: &str| {
            flag.or_else(|| std::env::var(env).ok()?.parse().ok())
                .map_or(0, |ms| i64::try_from(ms).unwrap_or(i64::MAX))
        };
        aws_dynamodb::Windows {
            create_ms: pick(self.dynamodb_create_ms, "DOZE_DDB_CREATE_MS"),
            update_ms: pick(self.dynamodb_update_ms, "DOZE_DDB_UPDATE_MS"),
            delete_ms: pick(self.dynamodb_delete_ms, "DOZE_DDB_DELETE_MS"),
        }
    }

    /// CloudFormation's configuration.
    pub(crate) fn cloudformation(&self) -> aws_cloudformation::Config {
        use aws_cloudformation::Unsupported;
        let mut overrides: Vec<(String, Unsupported)> = self
            .cfn_skip
            .iter()
            .map(|t| (t.clone(), Unsupported::Skip))
            .collect();
        overrides.extend(self.cfn_fail.iter().map(|t| (t.clone(), Unsupported::Fail)));
        aws_cloudformation::Config {
            unsupported: match self.cfn_unsupported {
                CfnUnsupported::Default => Unsupported::Default,
                CfnUnsupported::Skip => Unsupported::Skip,
                CfnUnsupported::Fail => Unsupported::Fail,
            },
            overrides,
            logical_names: self.cfn_logical_names,
        }
    }
}
