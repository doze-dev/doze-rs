//! Every server setting, once: its flag, its `DOZE_AWS_*` variable, its `doze-aws.toml` key (all the same name), its default and
//! its help. The command line, the environment, the file and `config print` are all made from this table, so they cannot drift.

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Text.
    Text,
    /// A whole number of at least this.
    Count(u64),
    /// On or off (a flag with no value on the command line).
    Switch,
    /// A list (comma-separated on the command line and in the environment, an array in the file).
    List,
    /// One of these words.
    Choice(&'static [&'static str]),
}

/// One setting.
#[derive(Debug)]
pub(crate) struct Key {
    /// The flag without its dashes, the file key, and (upper-cased, `-` as `_`, after `DOZE_AWS_`) the variable.
    pub(crate) name: &'static str,
    /// What it takes.
    pub(crate) kind: Kind,
    /// Its default, as written in the file (a list's items comma-separated).
    pub(crate) default: &'static str,
    /// The placeholder in help (`ADDR`).
    pub(crate) value_name: &'static str,
    /// One line of help.
    pub(crate) help: &'static str,
    /// The heading it is listed under.
    pub(crate) group: &'static str,
}

const fn key(
    name: &'static str,
    kind: Kind,
    default: &'static str,
    value_name: &'static str,
    help: &'static str,
    group: &'static str,
) -> Key {
    Key {
        name,
        kind,
        default,
        value_name,
        help,
        group,
    }
}

const SERVER: &str = "Server";
const ACCESS: &str = "Access";
const SERVICES: &str = "Services";

/// Every setting, in the order help and `config print` list them.
pub(crate) const KEYS: &[Key] = &[
    key(
        "listen",
        Kind::Text,
        "127.0.0.1:4566",
        "ADDR",
        "Where to listen (port 0 picks a free one)",
        SERVER,
    ),
    key(
        "data-dir",
        Kind::Text,
        "./doze-aws-data",
        "DIR",
        "Where state is kept (in the file: relative to the file)",
        SERVER,
    ),
    key(
        "region",
        Kind::Text,
        "us-east-1",
        "REGION",
        "The region `doze-aws env` points at, and that requests naming none (unsigned S3) are for",
        SERVER,
    ),
    key(
        "account",
        Kind::Text,
        "000000000000",
        "ID",
        "The twelve-digit account every request is for",
        SERVER,
    ),
    key(
        "services",
        Kind::List,
        "all",
        "LIST",
        "The services to serve (comma-separated); the rest answer with how to turn them on",
        SERVER,
    ),
    key(
        "template",
        Kind::Text,
        "",
        "FILE",
        "A CloudFormation template to apply once listening (as `doze-aws apply` would)",
        SERVER,
    ),
    key(
        "auth",
        Kind::Choice(&["trust", "verify"]),
        "trust",
        "MODE",
        "trust: route by the credential scope, accept any key; verify: check every signature",
        ACCESS,
    ),
    key(
        "access-key",
        Kind::Text,
        "test",
        "KEY",
        "The access key `--auth verify` accepts",
        ACCESS,
    ),
    key(
        "secret-key",
        Kind::Text,
        "test",
        "KEY",
        "The secret key `--auth verify` accepts",
        ACCESS,
    ),
    key(
        "iam",
        Kind::Choice(&["off", "soft", "enforce"]),
        "soft",
        "MODE",
        "What IAM does with a request its policies refuse: off, soft (log it, let it through), enforce",
        ACCESS,
    ),
    key(
        "idle-secs",
        Kind::Count(0),
        "300",
        "SECS",
        "Seconds without a request before a service lets go of its memory and files",
        SERVICES,
    ),
    key(
        "lambda-idle-secs",
        Kind::Count(0),
        "600",
        "SECS",
        "Seconds a warm Lambda process waits for its next invocation",
        SERVICES,
    ),
    key(
        "lambda-quiet",
        Kind::Switch,
        "false",
        "",
        "Keep what Lambda functions print out of this log (it is always in CloudWatch Logs)",
        SERVICES,
    ),
    key(
        "logs-retention-days",
        Kind::Count(0),
        "7",
        "DAYS",
        "Days a log event is kept locally whatever its group's retention (0 keeps them)",
        SERVICES,
    ),
    key(
        "logs-max-events",
        Kind::Count(0),
        "100000",
        "N",
        "The most events one log group keeps locally (0 is no cap)",
        SERVICES,
    ),
    key(
        "cloudwatch-retention-hours",
        Kind::Count(1),
        "24",
        "HOURS",
        "Hours CloudWatch metric data is kept",
        SERVICES,
    ),
    key(
        "cfn-unsupported",
        Kind::Choice(&["default", "skip", "fail"]),
        "default",
        "MODE",
        "What CloudFormation does with resource types doze-aws does not make",
        SERVICES,
    ),
    key(
        "cfn-skip",
        Kind::List,
        "",
        "TYPE",
        "Resource types CloudFormation accepts unmade whatever their class",
        SERVICES,
    ),
    key(
        "cfn-fail",
        Kind::List,
        "",
        "TYPE",
        "Resource types CloudFormation refuses whatever their class",
        SERVICES,
    ),
    key(
        "cfn-logical-names",
        Kind::Switch,
        "false",
        "",
        "Name resources a template leaves unnamed after their logical id",
        SERVICES,
    ),
    key(
        "sqs-skip-60s-waits",
        Kind::Switch,
        "false",
        "",
        "Let a deleted SQS queue's name be reused, and a queue be purged again, at once (AWS makes both wait 60 seconds)",
        SERVICES,
    ),
    key(
        "dynamodb-create-ms",
        Kind::Count(0),
        "0",
        "MS",
        "Milliseconds a new DynamoDB table or index stays CREATING (AWS takes seconds; 0 lets a waiter pass at once)",
        SERVICES,
    ),
    key(
        "dynamodb-update-ms",
        Kind::Count(0),
        "0",
        "MS",
        "Milliseconds a DynamoDB table stays UPDATING after UpdateTable",
        SERVICES,
    ),
    key(
        "dynamodb-delete-ms",
        Kind::Count(0),
        "0",
        "MS",
        "Milliseconds a DynamoDB table stays DELETING after DeleteTable",
        SERVICES,
    ),
];

/// The setting called `name`.
pub(crate) fn find(name: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.name == name)
}

/// The environment variable for `name`: `DOZE_AWS_DATA_DIR` for `data-dir`.
pub(crate) fn env_var(name: &str) -> String {
    format!("DOZE_AWS_{}", name.to_ascii_uppercase().replace('-', "_"))
}

/// The setting nearest to an unknown `name`, if one is near.
pub(crate) fn nearest(name: &str) -> Option<&'static str> {
    KEYS.iter()
        .map(|k| (strsim::jaro_winkler(k.name, name), k.name))
        .filter(|(score, _)| *score > 0.8)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| n)
}
