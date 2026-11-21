//! The leaf types of a descriptor: how a text is read, how an id is had, a call, a column, a field, a form and its actions.
//! [`super::Descriptor`], [`super::Kind`] and what a kind's pages may call stay in the parent module.

use super::{Pending, path};

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
    /// They are the same, but for the rows the service lists without an id of their own ([`Pending`]).
    Pending(&'static Pending),
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
    /// The cells are states (`ACTIVE`, `ALARM`): the page shows them as badges.
    pub badge: bool,
    /// What an absent value means, shown in its place (`Indefinite`); empty for a dash.
    pub empty: &'static str,
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
    /// A value here means the resource is on its way out (a secret marked for deletion): its page offers neither reveal nor
    /// delete, and a value that was to be revealed is not hidden but said to be unreadable (`empty`).
    pub retires: bool,
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
