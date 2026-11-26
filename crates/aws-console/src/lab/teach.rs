//! The words of the Lab: what each rehearsal stands for in production, what to expect, what a well-behaved client does, the SDK
//! settings that decide how it goes (with their real defaults: the `settings` module) and where to see it. One module, served by
//! `GET /api/v1/lab/guide`; the recipes that name a person's own queues and tables are made by the page.

use crate::model::{LabGuide, LabIntro, LabSetting, LabStep, LabTeach};

mod settings;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

const WATCH: &str = "On Live, each call the rule touched carries a tag with the rule's number and name (a slowed call says its time includes the wait). Filter Live by the rule, or to the calls of the Lab only; the rule's own page lists them and counts its hits.";

#[allow(clippy::too_many_arguments)] // one argument for each part of a lesson
fn lesson(
    tab: &str,
    title: &str,
    lede: &str,
    simulates: &str,
    expect: &str,
    client: &[&str],
    settings: Vec<LabSetting>,
    watch: &str,
) -> LabTeach {
    LabTeach {
        tab: tab.to_owned(),
        title: title.to_owned(),
        lede: lede.to_owned(),
        simulates: simulates.to_owned(),
        expect: expect.to_owned(),
        client: strings(client),
        settings,
        watch: watch.to_owned(),
    }
}

/// How the Lab works.
#[must_use]
pub fn intro() -> LabIntro {
    let step = |title: &str, text: &str| LabStep {
        title: title.to_owned(),
        text: text.to_owned(),
    };
    LabIntro {
        what: "The Lab makes the calls your apps send to doze-aws slow, fail the way AWS fails, or drop, for a while, so you see what your client does before production does it to you.".to_owned(),
        steps: vec![
            step("Pick a rehearsal", "Throttle, unavailable, internal error, access denied, slow or drop. Each has its lesson: what it stands for, what to expect, what a well-behaved client does."),
            step("Say which calls", "Name a service, operations, a queue, table or bucket (a name or a pattern with * and ?), a region or an access key. A field left empty fits anything. Choose how many of the calls (every one, 1 in N, or a chance) and for how long."),
            step("Watch it", "Run your app. Live tags every call the rule touched, and the rule counts its hits. The rule ends by itself; Stop all ends every rule now."),
        ],
        limits: strings(&[
            "Rules live in memory: a restart clears them, and only this console can make one.",
            "A fault never reaches storage: a failed call writes nothing, and a dropped one is a closed connection.",
            "Not bent: this console's own calls (Send a call too), calls between services (SNS delivering to SQS), function URLs, API Gateway stages and the /_doze/ addresses.",
            "At most 50 rules at once; every rule ends within 24 hours.",
        ]),
    }
}

fn throttle() -> LabTeach {
    lesson(
        "throttle",
        "Throttle",
        "Answers some of your calls with the service's own throttling error, and never asks the service.",
        "AWS refusing calls over a rate limit: a hot DynamoDB partition or a table out of capacity, an S3 prefix that has not scaled up yet, a Lambda function out of concurrency, Secrets Manager or SQS asked too often. Each service says it in its own words: SQS RequestThrottled, DynamoDB and Kinesis ProvisionedThroughputExceededException, S3 SlowDown (503), SNS Throttled (429), Lambda TooManyRequestsException (429), most others ThrottlingException.",
        "Each call the rule fits is answered at once with that service's error, and nothing is written. The SDKs retry a throttle with a longer wait than other errors, so you see the call on Live, then its retries, each tagged while the rule still fits; when the SDK gives up, your code gets the error. A rule that fits every call throttles the retries too: use 1 in N to see a retry succeed.",
        &[
            "Retries with exponential backoff and jitter, with a cap on attempts. The SDK does this: do not put a second retry loop around it.",
            "Treats the error that is left after the retries as 'try again later': the message stays in its queue, the item stays in its buffer. It is not dropped.",
            "Makes a retried write safe to repeat (an idempotency key, a conditional write): the SDK may send it twice.",
            "Slows down when it is throttled instead of waiting for the next refusal: batches, a queue in front, keys spread over prefixes and partitions.",
        ],
        settings::retries(),
        WATCH,
    )
}

fn unavailable() -> LabTeach {
    lesson(
        "unavailable",
        "Unavailable",
        "Answers some of your calls with a 503, the service is unavailable, and never asks the service.",
        "A service, or something behind it, briefly out: a regional event, a deployment, a dependency that timed out. In AWS it is a 503 ServiceUnavailable (API Gateway says ServiceUnavailableException).",
        "Each call the rule fits is answered 503 at once and nothing is written. Every SDK retries a 503 as a transient error, with its short wait: expect a quick run of retries on Live, and the error in your code when they are spent.",
        &[
            "Retries a 503 with backoff, and gives up after a few attempts: an outage is not a reason to hold a thread for ever.",
            "Keeps working for the user where it can: serves what it has cached, queues the work, says it will try again.",
            "Stops sending for a while when most calls fail (a circuit breaker), then tries one call to see if the service is back.",
            "Does not count the work as lost: it can be sent again when the service is back.",
        ],
        settings::retries(),
        WATCH,
    )
}

fn internal() -> LabTeach {
    lesson(
        "internal",
        "Internal error",
        "Answers some of your calls with a 500, the service failed inside, and never asks the service.",
        "A failure on the service's side that your request did not cause. In AWS it is a 500 and the service's own name for it: InternalFailure, DynamoDB InternalServerError, S3 InternalError, Kinesis InternalFailureException, Lambda ServiceException. It is rare, and a retry usually works.",
        "Each call the rule fits is answered 500 at once and nothing is written. Every SDK retries a 500 as a transient error: expect the call and its retries on Live, and the error in your code when the attempts are spent.",
        &[
            "Retries, with backoff, a call that is safe to repeat; the SDK does it for reads and, with an idempotency key, for writes.",
            "Does not treat a 500 as bad input: nothing about the request needs to change.",
            "Logs the request id the answer carries, so a report to AWS Support can name the call.",
            "Raises an alarm when 500s keep coming, not for one.",
        ],
        settings::retries(),
        WATCH,
    )
}

fn denied() -> LabTeach {
    lesson(
        "access-denied",
        "Access denied",
        "Answers some of your calls as if the caller's policies refused them, and never asks the service.",
        "A policy saying no: a role that lost an action, a bucket policy or a key policy that changed, a permissions boundary, a service control policy. It is not an outage. In AWS it is AccessDeniedException (S3 says AccessDenied, SNS says AuthorizationError).",
        "Each call the rule fits is answered with the denial at once and nothing is written. No SDK retries it: it is in none of their retry lists, so your code gets the error on the first attempt, and Live shows it as denied. To see doze-aws's own IAM refuse a call by the caller's real policies, start with --iam enforce and open the Access page.",
        &[
            "Does not retry: the call will fail again until a person changes a policy.",
            "Puts the action, the resource and the request id from the message in its log, so the person who fixes it knows what to allow.",
            "Fails the work item where someone will see it (a dead-letter queue, an alert) instead of looping on it.",
            "Fails clearly at start-up where it can: a service that cannot reach what it needs says so before it takes traffic.",
        ],
        Vec::new(),
        WATCH,
    )
}

fn slow() -> LabTeach {
    lesson(
        "slow",
        "Slow",
        "Makes the calls the rule fits wait before they are served.",
        "A service answering slowly: a hot partition, a cold Lambda start, a congested network, a big payload, a dependency that is itself slow. The call works, late.",
        "Each call the rule fits waits the delay (and a random part of the jitter) before it reaches the service, and is then served as usual; its time on Live includes the wait. A call that takes longer than your client's timeout fails on the client, and the SDK retries it: a client that gives up while the call waits is never served here, where in production the service may have done the work.",
        &[
            "Sets timeouts shorter than the SDK's defaults, for the connection and for the whole call, so a slow call fails early and can be retried.",
            "Retries only calls that are safe to repeat, and bounds the time of all the retries together.",
            "Does not hold a request thread, or a user, on one slow call: runs it in the background or shows progress.",
            "Moves on without the call when it can (a recommendation that is late is left out).",
        ],
        settings::timeouts(),
        WATCH,
    )
}

fn drop_rule() -> LabTeach {
    lesson(
        "drop",
        "Drop",
        "Closes the connection of the calls the rule fits, with no answer at all.",
        "A connection that dies: a load balancer or NAT that forgets an idle connection, a server that crashes mid-request, a network that blips. The client gets no HTTP status, only a connection error.",
        "The call is read, judged, and the connection is closed without a byte of answer; the call never reaches the service, so nothing is written, and Live shows it with no status. The SDK sees a connection error and retries it on a new connection. In production a closed connection can come after the service wrote: here it never does.",
        &[
            "Retries with backoff: a connection error is transient, and the SDK does it.",
            "Does not assume the write failed. After a closed connection a call may or may not have happened: make writes safe to repeat.",
            "Does not keep a pool of dead connections: the next call opens a new one.",
        ],
        settings::retries(),
        WATCH,
    )
}

/// One rehearsal for each tab of the form, in its order.
#[must_use]
pub fn rehearsals() -> Vec<LabTeach> {
    vec![
        throttle(),
        unavailable(),
        internal(),
        denied(),
        slow(),
        drop_rule(),
    ]
}

/// `GET /api/v1/lab/guide`.
#[must_use]
pub fn guide() -> LabGuide {
    LabGuide {
        intro: intro(),
        rehearsals: rehearsals(),
    }
}

#[cfg(test)]
mod tests;
