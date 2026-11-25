//! The SDK settings the lessons name, with the defaults the SDKs ship with. Every number here was read from the SDK's own
//! documentation or source (2026-11-25); a default that could not be confirmed is not given a number.
//!
//! - botocore (Python): the `botocore.config.Config` reference (`connect_timeout`, `read_timeout` 60 s; `standard` is the default
//!   mode with 3 attempts, 4 for DynamoDB and DynamoDB Streams), the boto3 retries guide (legacy mode 5 attempts; standard backoff
//!   base 2, at most 20 s, from 50 ms for transient errors, 25 ms for DynamoDB, 1 s for throttling), `botocore/data/_retry.json`.
//! - AWS SDKs and Tools reference guide, *Retry behavior*: `AWS_RETRY_MODE`, `AWS_MAX_ATTEMPTS`, standard mode by default, 3
//!   attempts (DynamoDB 4), a retry token bucket of 500 tokens, the transient and throttling error lists.
//! - JavaScript v3: `@smithy/node-http-handler` (`requestTimeout` defaults to 0, which is no timeout; a connection timeout of 0 or
//!   unset sets none).
//! - Java 2.x developer guide, *Configure retry behavior*: standard is the default strategy from 2.58.0 (3 attempts, 4 for
//!   DynamoDB; before it, legacy with 4, and 9 for DynamoDB). The page gives no default for `apiCallTimeout` or
//!   `apiCallAttemptTimeout`.
//! - Go v2 developer guide, *Retries and timeouts*: `retry.Standard`, 3 attempts, 20 s most backoff, a rate limiter of 500; timeouts
//!   come from the `context` the call is given.

use crate::model::LabSetting;

fn setting(sdk: &str, name: &str, default: &str, why: &str) -> LabSetting {
    LabSetting {
        sdk: sdk.to_owned(),
        name: name.to_owned(),
        default: default.to_owned(),
        why: why.to_owned(),
    }
}

const BOTO: &str = "boto3 and the AWS CLI (botocore)";
const JS: &str = "AWS SDK for JavaScript v3";
const JAVA: &str = "AWS SDK for Java 2.x";
const GO: &str = "AWS SDK for Go v2";

/// How a client retries: what a throttle, an unavailable and an internal error meet.
pub(super) fn retries() -> Vec<LabSetting> {
    vec![
        setting(
            BOTO,
            "retry_mode (AWS_RETRY_MODE; Config(retries={'mode': ...}))",
            "standard (legacy and adaptive exist)",
            "Standard retries throttling errors, 500, 502, 503 and 504, timeouts and connection errors; it gives up when its retry budget is spent.",
        ),
        setting(
            BOTO,
            "max_attempts (AWS_MAX_ATTEMPTS; Config(retries={'total_max_attempts': ...}))",
            "3 attempts in all, the first included (4 for DynamoDB and DynamoDB Streams); 5 in legacy mode",
            "After this many attempts your code gets the error.",
        ),
        setting(
            BOTO,
            "backoff (not set by you)",
            "exponential, base 2, at most 20 s; starts at 1 s for a throttle and 50 ms for other errors (25 ms for DynamoDB)",
            "A throttle waits longer than a 500: its first retry waits up to a second.",
        ),
        setting(
            JS,
            "maxAttempts (AWS_MAX_ATTEMPTS)",
            "3, the first included",
            "The retry count of the client; a per-client setting.",
        ),
        setting(
            JS,
            "retryMode (AWS_RETRY_MODE)",
            "standard",
            "Standard or adaptive; adaptive also slows the client down when it is throttled.",
        ),
        setting(
            JAVA,
            "retryStrategy (AWS_RETRY_MODE, aws.retryMode)",
            "standard from 2.58.0, with 3 attempts (4 for DynamoDB); before it, legacy with 4 (9 for DynamoDB)",
            "Which strategy and how many attempts; the standard one waits from 50 ms (1 s for a throttle), at most 20 s.",
        ),
        setting(
            GO,
            "retry.Standard: MaxAttempts, MaxBackoff",
            "3 attempts, 20 s most backoff",
            "A rate limiter of 500 tokens stops retries when many calls fail.",
        ),
    ]
}

/// How long a client waits before it gives up: what a slow answer meets.
pub(super) fn timeouts() -> Vec<LabSetting> {
    vec![
        setting(
            BOTO,
            "connect_timeout and read_timeout (botocore.config.Config)",
            "60 s each",
            "A call slower than read_timeout fails on the client with a timeout error, which is retried.",
        ),
        setting(
            JS,
            "requestTimeout and connectionTimeout (NodeHttpHandler)",
            "requestTimeout 0, which is no timeout; no connection timeout unless you set one",
            "By default a slow call is waited for as long as it takes: set a timeout, or nothing stops a stuck call.",
        ),
        setting(
            JAVA,
            "apiCallTimeout and apiCallAttemptTimeout (ClientOverrideConfiguration)",
            "no default is documented: set both",
            "The first bounds the call with its retries, the second one attempt.",
        ),
        setting(
            GO,
            "the context of the call (context.WithTimeout)",
            "none: the SDK has no timeout of its own for a call",
            "A call with a context that never ends waits for as long as the answer takes.",
        ),
    ]
}
