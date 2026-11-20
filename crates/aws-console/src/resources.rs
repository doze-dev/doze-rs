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

mod dynamodb;
mod kms;
mod lambda;
mod s3;
mod secrets;
mod sns;
mod sqs;
mod ssm;

/// One service's resources.
#[derive(Debug)]
pub struct Descriptor {
    /// The service's name in `--services` and in the console's addresses (`secretsmanager`).
    pub service: &'static str,
    /// Its name on the peer bus (its signing name).
    pub target: &'static str,
    /// It speaks a REST protocol (S3, Lambda): called with [`aws_peers::Peers::call_rest_as`].
    pub rest: bool,
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
}

/// A text read from an answer.
#[derive(Debug, Clone, Copy)]
pub struct Text {
    pub path: &'static str,
    /// Read the name out of a URL or ARN ([`path::tail`]).
    pub tail: bool,
}

impl Text {
    /// The value at `path`, as it is.
    #[must_use]
    pub const fn at(path: &'static str) -> Self {
        Self { path, tail: false }
    }

    /// The name in the URL or ARN at `path`.
    #[must_use]
    pub const fn tail(path: &'static str) -> Self {
        Self { path, tail: true }
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

/// One call: an operation of the descriptor's service (or of `service`, when set) and its body.
#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub op: &'static str,
    /// JSON with placeholders (module text).
    pub body: &'static str,
    /// Another service's operation (a queue's Lambda triggers are Lambda's).
    pub service: Option<&'static str>,
    /// A refusal is not a failure of the page (a bucket with no policy answers `NoSuchBucketPolicy`).
    pub optional: bool,
}

impl Step {
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
    /// `yes` when the value is `true`, `no` otherwise (absent included).
    Flag(&'static str, &'static str),
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

/// A field with no link that is never hidden.
#[must_use]
pub(crate) const fn field(label: &'static str, path: &'static str, format: Format) -> Field {
    Field {
        label,
        path,
        format,
        link: &[],
        hide_when: None,
    }
}

/// A column.
#[must_use]
pub(crate) const fn col(title: &'static str, path: &'static str, format: Format) -> Column {
    Column {
        title,
        path,
        format,
    }
}

/// An input that is not required, empty at first.
#[must_use]
pub(crate) const fn input(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    kind: InputKind,
) -> Input {
    Input {
        key,
        label,
        help,
        kind,
        required: false,
        placeholder: "",
        value: "",
    }
}
