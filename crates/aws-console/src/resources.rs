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
pub mod blockers;
pub mod bus;
pub mod catalog;
pub mod deep;
pub mod ops;
pub mod page;
pub mod params;
pub mod path;
pub mod run;
pub mod says;
pub mod tags;
pub mod view;

mod bare;
mod parts;
mod pending;
pub use pending::Pending;
mod pretty;
mod shape;
pub use crate::model::Needs;
pub use blockers::{Batch, Blocker, Clear, Kept};
pub use page::{Page, Read, Verb};
pub use params::{Apply, Options, Param, ParamKind};
pub(crate) use parts::{col, col_or, field, input, or, state};
pub use shape::*;
pub use tags::{Shape, Tags, Write};

mod apigateway;
mod cfn_kept;
mod cloudformation;
mod cloudwatch;
mod dynamodb;
mod eventbridge;
mod eventbridge_blockers;
mod iam;
mod iam_blockers;
mod iam_roles;
mod kinesis;
mod kms;
mod lambda;
mod logs;
mod s3;
mod s3_ops;
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
    /// Its pages of its own (a queue's messages, a bucket's objects, a table's items): tabs of the detail page, drawn by the
    /// console's app, each with the operations it may make on the resource.
    pub deep: &'static [Deep],
    /// Operations of the kind that no resource is bound to (a create with a form of its own, a bulk delete): `POST
    /// …/<kind>/op/<Operation>` ([`ops`]).
    pub ops: &'static [Op],
    /// How a resource of the kind is tagged ([`tags`]).
    pub tags: Option<Tags>,
    /// What stands in the way of deleting one, and what a delete leaves behind ([`blockers`]).
    pub blockers: &'static [Blocker],
    pub kept: &'static [Kept],
    /// What the person types to confirm a delete, when it is not the resource's name (`delete`).
    pub confirm: &'static str,
    /// The list's filters and selectors ([`params`]).
    pub params: &'static [Param],
    /// How a detail page speaks of a resource: its sentence, badges and verbs ([`page`]).
    pub page: Page,
}

/// A page of one resource that its descriptor's generic detail cannot draw: a tab of the detail page that the console's app
/// draws by its `id`, and the only operations it may make, each on this resource (`POST …/<name>/op/<Operation>`).
#[derive(Debug)]
pub struct Deep {
    /// In the address (`?tab=messages`) and in the app's table of pages.
    pub id: &'static str,
    pub title: &'static str,
    pub ops: &'static [Op],
}

/// An operation a [`Deep`] page may make. The page sends the input; the members of `bind` are then set over it, filled as a
/// step's body (`{"QueueUrl":"{id}"}`), so the call acts on this resource and on no other. A member bound to `null` is taken
/// out of the input (another way to name a resource: `logGroupIdentifier` beside `logGroupName`).
#[derive(Debug, Clone, Copy)]
pub struct Op {
    pub op: &'static str,
    pub bind: &'static str,
    /// For an operation on a part of the resource that the page names (an execution of a state machine): the member, and
    /// what its value must begin with, filled as `bind` is (`arn:aws:states:{region}:{account}:execution:{name}:`).
    pub within: Option<(&'static str, &'static str)>,
    /// For an operation on a part the page names by a handle no prefix tells (a move task's handle): how to list this
    /// resource's own parts, among which the handle must be ([`deep::Owned`]).
    pub owned: Option<&'static deep::Owned>,
    /// Another service's operation, by its name on the bus (a queue's `Subscribe` is `sns`'s); the descriptor's own when absent.
    pub service: Option<&'static str>,
}

/// An operation of a [`Deep`] page.
#[must_use]
pub const fn op(op: &'static str, bind: &'static str) -> Op {
    Op {
        op,
        bind,
        within: None,
        owned: None,
        service: None,
    }
}

/// An operation of a [`Deep`] page on a part of the resource: the page names it by `member`, whose value must begin with
/// `prefix` (filled).
#[must_use]
pub const fn op_within(op: &'static str, member: &'static str, prefix: &'static str) -> Op {
    Op {
        op,
        bind: "{}",
        within: Some((member, prefix)),
        owned: None,
        service: None,
    }
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
