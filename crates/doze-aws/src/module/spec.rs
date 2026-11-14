//! The `aws` block: the Go module's (`doze-aws/module/config.go`), argument for argument, so a doze.hcl written for it
//! decodes here unchanged. Each nested block is one resource of the stack the module keeps (`template.rs`).

use std::collections::BTreeMap;

use doze_module::Spec;

/// The `aws` block.
#[derive(Debug, Default, Spec)]
pub(crate) struct AwsSpec {
    #[spec(block, doc = "One S3 bucket (the label is the bucket name).")]
    pub(crate) bucket: Vec<Bucket>,
    #[spec(
        block,
        doc = "One SQS queue; dlq = \"auto\" creates and wires a dead-letter companion."
    )]
    pub(crate) queue: Vec<Queue>,
    #[spec(
        block,
        doc = "One SNS topic with subscribe blocks (queue, lambda or http; filter, raw)."
    )]
    pub(crate) topic: Vec<Topic>,
    #[spec(
        block,
        doc = "One DynamoDB table; key uses the \"pk:S sk:N\" shorthand, plus gsi and lsi blocks."
    )]
    pub(crate) table: Vec<Table>,
    #[spec(
        block,
        doc = "One Lambda function running as a real local process (code = a local directory)."
    )]
    pub(crate) function: Vec<Function>,
    #[spec(
        block,
        doc = "One EventBridge rule; targets are \"queue:name\", \"topic:name\" or \"lambda:name\"."
    )]
    pub(crate) rule: Vec<Rule>,
    #[spec(block, doc = "One KMS key with real local crypto.")]
    pub(crate) key: Vec<Key>,
    #[spec(block, doc = "One Secrets Manager secret.")]
    pub(crate) secret: Vec<Secret>,
    #[spec(block, doc = "One SSM parameter (the label is the full /path).")]
    pub(crate) parameter: Vec<Parameter>,
    #[spec(
        block,
        doc = "One Step Functions state machine, run locally against the stack's functions, queues and topics."
    )]
    pub(crate) state_machine: Vec<StateMachine>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Bucket {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(default = false, doc = "Enable object versioning.")]
    pub(crate) versioning: bool,
    #[spec(default = false, doc = "Enable object lock (implies versioning).")]
    pub(crate) object_lock: bool,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
    #[spec(block, doc = "Send the bucket's events to a queue, topic or function.")]
    pub(crate) notify: Vec<Notify>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Notify {
    #[spec(doc = "S3 event names; all object-created events when left out.")]
    pub(crate) events: Vec<String>,
    #[spec(doc = "Only keys with this prefix.")]
    pub(crate) prefix: Option<String>,
    #[spec(doc = "Only keys with this suffix.")]
    pub(crate) suffix: Option<String>,
    #[spec(doc = "A queue of this block to send to.")]
    pub(crate) queue: Option<String>,
    #[spec(doc = "A topic of this block to send to.")]
    pub(crate) topic: Option<String>,
    #[spec(doc = "A function of this block to invoke.")]
    pub(crate) lambda: Option<String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Queue {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(default = false, doc = "FIFO queue (its name gets .fifo).")]
    pub(crate) fifo: bool,
    #[spec(default = false, doc = "Content-based deduplication (FIFO only).")]
    pub(crate) content_dedup: bool,
    #[spec(doc = "\"auto\" or the name of another queue of this block.")]
    pub(crate) dlq: Option<String>,
    #[spec(doc = "Receives before a message is dead-lettered (needs dlq).")]
    pub(crate) max_receives: Option<i64>,
    #[spec(doc = "Visibility timeout, seconds.")]
    pub(crate) visibility: Option<i64>,
    #[spec(doc = "Delivery delay, seconds.")]
    pub(crate) delay: Option<i64>,
    #[spec(doc = "Message retention, seconds.")]
    pub(crate) retention: Option<i64>,
    #[spec(doc = "Long-poll wait, seconds.")]
    pub(crate) receive_wait: Option<i64>,
    #[spec(doc = "Largest message, bytes.")]
    pub(crate) max_size: Option<i64>,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Topic {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
    #[spec(block, doc = "A subscription: exactly one of queue, lambda or http.")]
    pub(crate) subscribe: Vec<Subscription>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Subscription {
    #[spec(doc = "A queue of this block.")]
    pub(crate) queue: Option<String>,
    #[spec(doc = "A function of this block.")]
    pub(crate) lambda: Option<String>,
    #[spec(doc = "An http(s) endpoint.")]
    pub(crate) http: Option<String>,
    #[spec(doc = "An SNS filter policy, JSON.")]
    pub(crate) filter: Option<String>,
    #[spec(default = false, doc = "Raw message delivery.")]
    pub(crate) raw: bool,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Table {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The key: \"pk:S\" or \"pk:S sk:N\" (types S, N or B).")]
    pub(crate) key: String,
    #[spec(doc = "The attribute holding each item's expiry time.")]
    pub(crate) ttl: Option<String>,
    #[spec(doc = "Refuse to delete the table.")]
    pub(crate) deletion_protection: Option<bool>,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
    #[spec(block, doc = "A global secondary index.")]
    pub(crate) gsi: Vec<Index>,
    #[spec(
        block,
        doc = "A local secondary index (its partition key is the table's)."
    )]
    pub(crate) lsi: Vec<Index>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Index {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The index's key, in the table's shorthand.")]
    pub(crate) key: String,
    #[spec(doc = "ALL (the default), KEYS_ONLY or INCLUDE.")]
    pub(crate) projection: Option<String>,
    #[spec(doc = "The attributes an INCLUDE projection carries.")]
    pub(crate) include: Vec<String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Function {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The runtime; provided.al2023 (a bootstrap in code) when left out.")]
    pub(crate) runtime: Option<String>,
    #[spec(doc = "The handler, for runtimes that take one.")]
    pub(crate) handler: Option<String>,
    #[spec(doc = "A local directory with the function's code, relative to doze.hcl.")]
    pub(crate) code: String,
    #[spec(doc = "Environment variables.")]
    pub(crate) env: BTreeMap<String, String>,
    #[spec(doc = "Timeout, seconds.")]
    pub(crate) timeout: Option<i64>,
    #[spec(doc = "Memory, MB.")]
    pub(crate) memory: Option<i64>,
    #[spec(doc = "Retries of an asynchronous invocation (0 to 2).")]
    pub(crate) retries: Option<i64>,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
    #[spec(block, doc = "Where a failed asynchronous invocation goes.")]
    pub(crate) dlq: Vec<Destination>,
    #[spec(
        block,
        doc = "Where a successful asynchronous invocation's result goes."
    )]
    pub(crate) on_success: Vec<Destination>,
    #[spec(block, doc = "Where a failed asynchronous invocation's result goes.")]
    pub(crate) on_failure: Vec<Destination>,
    #[spec(block, doc = "A queue of this block that invokes the function.")]
    pub(crate) trigger: Vec<Trigger>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Destination {
    #[spec(doc = "A queue of this block.")]
    pub(crate) queue: Option<String>,
    #[spec(doc = "A topic of this block.")]
    pub(crate) topic: Option<String>,
    #[spec(doc = "A function of this block.")]
    pub(crate) lambda: Option<String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Trigger {
    #[spec(doc = "A queue of this block.")]
    pub(crate) queue: String,
    #[spec(doc = "Messages per invocation.")]
    pub(crate) batch: Option<i64>,
    #[spec(doc = "Whether the trigger runs.")]
    pub(crate) enabled: Option<bool>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Rule {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The event bus; the default bus when left out.")]
    pub(crate) bus: Option<String>,
    #[spec(doc = "An event pattern, JSON.")]
    pub(crate) pattern: Option<String>,
    #[spec(doc = "A schedule: rate(...) or cron(...).")]
    pub(crate) schedule: Option<String>,
    #[spec(doc = "Whether the rule runs.")]
    pub(crate) enabled: Option<bool>,
    #[spec(doc = "\"queue:name\", \"topic:name\" or \"lambda:name\", each of this block.")]
    pub(crate) targets: Vec<String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Key {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The key spec; SYMMETRIC_DEFAULT when left out.")]
    pub(crate) spec: Option<String>,
    #[spec(doc = "The key usage; ENCRYPT_DECRYPT when left out.")]
    pub(crate) usage: Option<String>,
    #[spec(doc = "A description.")]
    pub(crate) description: Option<String>,
    #[spec(default = false, doc = "Rotate the key yearly.")]
    pub(crate) rotation: bool,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Secret {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The secret's value, as text.")]
    pub(crate) value: Option<String>,
    #[spec(doc = "The secret's value as base64, for binary secrets.")]
    pub(crate) binary: Option<String>,
    #[spec(doc = "A description.")]
    pub(crate) description: Option<String>,
    #[spec(
        default = false,
        doc = "Accepted for the Go module's blocks; the declared value is written whenever the declaration changes."
    )]
    #[expect(
        dead_code,
        reason = "read by the schema only: Go's blocks set it, and a stack has no 'stomp' to guard"
    )]
    pub(crate) force: bool,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct Parameter {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The parameter's value.")]
    pub(crate) value: String,
    #[spec(rename = "type", doc = "String (the default) or StringList.")]
    pub(crate) kind: Option<String>,
    #[spec(doc = "A description.")]
    pub(crate) description: Option<String>,
    #[spec(
        default = false,
        doc = "Accepted for the Go module's blocks; the declared value is written whenever the declaration changes."
    )]
    #[expect(
        dead_code,
        reason = "read by the schema only: Go's blocks set it, and a stack has no 'stomp' to guard"
    )]
    pub(crate) force: bool,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
}

#[derive(Debug, Default, Spec)]
pub(crate) struct StateMachine {
    #[spec(label)]
    pub(crate) name: String,
    #[spec(doc = "The States Language document: a heredoc, or file(\"./flow.asl.json\").")]
    pub(crate) definition: String,
    #[spec(doc = "Role ARN; the API requires one and nothing local evaluates it, so it defaults.")]
    pub(crate) role: Option<String>,
    #[spec(rename = "type", doc = "STANDARD (the default) or EXPRESS.")]
    pub(crate) kind: Option<String>,
    #[spec(doc = "Tags.")]
    pub(crate) tags: BTreeMap<String, String>,
}
