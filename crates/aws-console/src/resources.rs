//! Resources: what each service holds, read and changed through the peer bus as the console's own caller.
//!
//! A service joins by a [`Descriptor`]: plain data naming its kinds of resource, the operation that lists each (and how it
//! pages), the columns, the operations a detail page reads and the fields it shows, and the actions (create with a small form,
//! delete with a confirmation, reveal of a hidden value), each mapped to the service's own operation. The generic pages render
//! any descriptor; nothing here knows a service beyond its descriptor.
//!
//! Every call is made by [`bus::Bus`] as the caller `console` (an access key no one issued, so IAM takes it for the account
//! root: under `--iam enforce` the console is allowed what the person running doze-aws is). Each one goes through the target's
//! own model and authorizer, and the Live record shows it with `console` as its caller. Nothing runs unless a page asks:
//! there is no timer and no cache.
//!
//! Bodies are JSON written in the descriptor with placeholders ([`path::fill`]): `{id}`, `{name}`, `{account}`, `{region}`,
//! `{token}`, `{out:<path>}` (what an earlier step answered), `{form:<key>}`, `{num:<key>}`, `{bool:<key>}`. A member whose
//! placeholder has no value is left out, and an object in a list that loses one is dropped (a sort key not given).

pub mod act;
pub mod bus;
pub mod catalog;
pub mod path;
pub mod run;
pub mod view;

mod parts;
pub(crate) use parts::{col, field, input, or};

mod apigateway;
mod cloudformation;
mod cloudwatch;
mod dynamodb;
mod eventbridge;
mod iam;
mod iam_roles;
mod kinesis;
mod kms;
mod lambda;
mod logs;
mod s3;
mod scheduler;
mod secrets;
mod sns;
mod sqs;
mod ssm;
mod stepfunctions;

/// One service's resources.
#[derive(Debug)]
pub struct Descriptor {
    /// The service's name in `--services` and in the console's addresses (`secretsmanager`).
    pub service: &'static str,
    /// Its name on the peer bus (its signing name). The bus knows whether it speaks a REST protocol.
    pub target: &'static str,
    pub kinds: &'static [Kind],
}

/// One kind of resource of a service (SNS has topics and subscriptions).
#[derive(Debug)]
pub struct Kind {
    /// In addresses: `queues`.
    pub id: &'static str,
    /// `Queues`.
    pub title: &'static str,
    /// `queue`.
    pub one: &'static str,
    /// One line on what it is, under the list's title.
    pub about: &'static str,
    /// The input members that name one of these in a call (`QueueUrl`), so Live's link to a call's resource finds its kind.
    pub members: &'static [&'static str],
    pub list: List,
    /// Why the console does not create one and what does (a function needs its code: the AWS CLI or a deploy tool); empty when
    /// it creates one or there is nothing to say.
    pub elsewhere: &'static str,
    /// Calls made for each listed row, whose answers fill more columns (SQS lists only URLs).
    pub each: &'static [Step],
    pub columns: &'static [Column],
    /// What the calls take, from the name in the address.
    pub id_of: Id,
    /// The calls a detail page makes, in order; their answers are merged and read by [`Kind::fields`].
    pub detail: &'static [Step],
    pub fields: &'static [Field],
    /// Lists that belong to one resource (a bucket's objects, a topic's subscriptions).
    pub subs: &'static [Sub],
    pub create: Option<Create>,
    pub delete: Option<Delete>,
    pub reveal: Option<Reveal>,
}

/// How a kind is listed.
#[derive(Debug)]
pub struct List {
    pub step: Step,
    /// Where the rows are in the answer (`QueueUrls`).
    pub items: &'static str,
    /// A row that is a bare value is read as an object with this one member (`QueueUrl`).
    pub item_as: &'static str,
    /// The member of the answer that carries the next page's token; empty when the operation has one page.
    pub token_out: &'static str,
    /// Where a row's id is (what calls take); its name (the address) and the label shown are read from the same place unless set.
    pub id: &'static str,
    pub name: Text,
    /// What the list shows a row as, when it is not its name.
    pub label: Option<Text>,
    /// Rows left out unless the person asks for them (AWS managed keys).
    pub split: Option<Split>,
}

/// The rows of a list set aside by default: those whose value at `path` (after [`Kind::each`]) is not `keep`. The index
/// counts the kept ones and says how many others there are.
#[derive(Debug)]
pub struct Split {
    pub path: &'static str,
    pub keep: &'static str,
    /// What the others are, in the plural (`AWS managed keys`).
    pub others: &'static str,
}

/// A text read from an answer.
#[derive(Debug, Clone, Copy)]
pub struct Text {
    pub path: &'static str,
    /// Read the name out of a URL or ARN ([`path::tail`]).
    pub tail: bool,
    /// When not empty: what follows this in the value (`:rule/` of a rule's ARN is `orders-bus/on-order`, or `on-order` on the
    /// default bus).
    pub after: &'static str,
}

impl Text {
    /// The value at `path`, as it is.
    #[must_use]
    pub const fn at(path: &'static str) -> Self {
        Self {
            path,
            tail: false,
            after: "",
        }
    }

    /// The name in the URL or ARN at `path`.
    #[must_use]
    pub const fn tail(path: &'static str) -> Self {
        Self {
            path,
            tail: true,
            after: "",
        }
    }

    /// What follows `marker` in the value at `path` (the value as it is when it has no `marker`).
    #[must_use]
    pub const fn after(path: &'static str, marker: &'static str) -> Self {
        Self {
            path,
            tail: false,
            after: marker,
        }
    }

    /// The text read out of `item`.
    #[must_use]
    pub fn read(self, item: &serde_json::Value) -> Option<String> {
        let t = path::text(item, self.path)?;
        Some(if self.tail {
            path::tail(&t)
        } else if !self.after.is_empty()
            && let Some((_, rest)) = t.split_once(self.after)
        {
            rest.to_owned()
        } else {
            t
        })
    }
}

/// How a resource's id (what its calls take) is had from its name.
#[derive(Debug)]
pub enum Id {
    /// They are the same.
    Name,
    /// Written from the name (`arn:aws:sns:{region}:{account}:{name}`).
    Template(&'static str),
    /// Asked for (SQS's `GetQueueUrl`), at `path` in the answer.
    Call(Step, &'static str),
}

/// One call: an operation of the descriptor's service (or of `service`, when set) and its body. A sub-list's step with no
/// body ([`Step::answered`]) makes no call: it reads what the detail's call `op` answered (a stack's outputs).
#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub op: &'static str,
    /// JSON with placeholders (module text).
    pub body: &'static str,
    /// Another service's operation, by its name on the bus (a queue's Lambda triggers are `lambda`'s; HTTP APIs are
    /// `apigatewayv2`).
    pub service: Option<&'static str>,
    /// A refusal is not a failure of the page (a bucket with no policy answers `NoSuchBucketPolicy`).
    pub optional: bool,
}

impl Step {
    /// No call: what the detail's call `op` answered (for a [`Sub`]).
    #[must_use]
    pub const fn answered(op: &'static str) -> Self {
        Self::new(op, "")
    }

    /// Whether this is no call of its own ([`Step::answered`]).
    #[must_use]
    pub const fn is_answered(&self) -> bool {
        self.body.is_empty()
    }

    #[must_use]
    pub const fn new(op: &'static str, body: &'static str) -> Self {
        Self {
            op,
            body,
            service: None,
            optional: false,
        }
    }

    /// The same call, to `service`.
    #[must_use]
    pub const fn of(mut self, service: &'static str) -> Self {
        self.service = Some(service);
        self
    }

    /// The same call, whose refusal is left out of the page.
    #[must_use]
    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// How a value is shown. The server writes `Flag` and `Tail` out; the page formats the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    /// Names, ids, ARNs, URLs.
    Mono,
    Number,
    Bytes,
    /// A time: seconds or milliseconds since 1970, or an ISO 8601 text.
    Time,
    Seconds,
    /// A JSON document, written out indented.
    Json,
    /// The name in a URL or ARN.
    Tail,
    /// A list of pairs (`[{"Name":"QueueName","Value":"orders"}]`, `[{"Key":..,"Value":..}]`) or an object of texts, written
    /// `QueueName=orders, env=dev`.
    Pairs,
    /// `yes` when the value is `true`, `no` otherwise (absent included).
    Flag(&'static str, &'static str),
    /// A value written as people say it (`CUSTOMER` is `Customer`); one not named, as it is.
    Named(&'static [(&'static str, &'static str)]),
}

/// A column of a list.
#[derive(Debug)]
pub struct Column {
    pub title: &'static str,
    pub path: &'static str,
    pub format: Format,
}

/// A field of a detail page.
#[derive(Debug)]
pub struct Field {
    pub label: &'static str,
    pub path: &'static str,
    pub format: Format,
    /// Where it points, the first whose condition holds.
    pub link: &'static [LinkTo],
    /// Hidden until revealed when the value at the path equals the text (`Parameter.Type` is `SecureString`); `("", "")` for
    /// always.
    pub hide_when: Option<(&'static str, &'static str)>,
    /// What an absent value means, shown in its place (`None` for a bucket with no policy, `us-east-1` for a bucket's empty
    /// location); empty for a dash.
    pub empty: &'static str,
}

/// A link from a value to a resource of the console: the name is the value's tail.
#[derive(Debug)]
pub struct LinkTo {
    pub service: &'static str,
    pub kind: &'static str,
    /// Only when the value at the path equals the text (an SNS endpoint is a queue when the protocol is `sqs`).
    pub when: Option<(&'static str, &'static str)>,
}

/// A list that belongs to one resource.
#[derive(Debug)]
pub struct Sub {
    pub title: &'static str,
    pub step: Step,
    pub items: &'static str,
    pub item_as: &'static str,
    pub columns: &'static [Column],
    /// Each row links to a resource of this service and kind, named by the text.
    pub link: Option<(&'static str, &'static str, Text)>,
    /// Where the answer says it has more than it gave.
    pub more: &'static str,
    /// Adds one from this page: the create of the service's kind `.0`, with its input `.1` filled with this resource's id (a
    /// subscription to this topic).
    pub add: Option<(&'static str, &'static str)>,
}

/// A form field of a create.
#[derive(Debug)]
pub struct Input {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: InputKind,
    pub required: bool,
    pub placeholder: &'static str,
    /// What it holds at first.
    pub value: &'static str,
}

/// What a form field takes.
#[derive(Debug)]
pub enum InputKind {
    Text,
    Number,
    Textarea,
    /// One of these.
    Select(&'static [&'static str]),
    /// On (`true`) or off (empty).
    Toggle,
}

/// How a resource is created.
#[derive(Debug)]
pub struct Create {
    pub inputs: &'static [Input],
    pub steps: &'static [Step],
    /// The new resource's name: the answer's value at `path`, else the form's `key`.
    pub name: Text,
    pub key: &'static str,
}

/// How a resource is deleted.
#[derive(Debug)]
pub struct Delete {
    /// `Delete`, or `Schedule deletion`.
    pub verb: &'static str,
    pub step: Step,
    /// What happens, said before it does.
    pub words: &'static str,
}

/// How a hidden value is revealed (a secret's value): one call, which shows in Live.
#[derive(Debug)]
pub struct Reveal {
    pub label: &'static str,
    pub step: Step,
    /// Where the value is in the answer; `a|b` for the first of two (a secret's text, else its bytes).
    pub path: &'static str,
}
