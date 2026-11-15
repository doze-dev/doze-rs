//! What CloudFormation keeps: stacks (with their resources and outputs), their events, and change sets. JSON values are kept as
//! text (the store's encoding has no place for an untyped value).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tmpl::params::Bound;

/// A stack.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Stack {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: Option<i64>,
    pub(crate) deleted_ms: Option<i64>,
    pub(crate) description: Option<String>,
    /// The template as it was given.
    pub(crate) body: String,
    /// The template after its transforms, as JSON text.
    pub(crate) processed: String,
    pub(crate) params: Vec<Bound>,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) capabilities: Vec<String>,
    pub(crate) notification_arns: Vec<String>,
    pub(crate) disable_rollback: bool,
    pub(crate) termination_protection: bool,
    pub(crate) role_arn: Option<String>,
    pub(crate) timeout: Option<i64>,
    pub(crate) policy: Option<String>,
    pub(crate) outputs: Vec<Output>,
    pub(crate) resources: BTreeMap<String, Res>,
    /// The id and type of the last operation (`CREATE_STACK`, `UPDATE_STACK`, ...).
    pub(crate) last_op: Option<(String, String)>,
    pub(crate) change_set_id: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) root_id: Option<String>,
    /// The exports this stack's template imports.
    pub(crate) imports: Vec<String>,
    /// The number of events written.
    pub(crate) events: u64,
    pub(crate) retain_except_on_create: bool,
    /// The template, processed template and parameters before the last update, kept until it ends well (a rollback goes back to
    /// them).
    pub(crate) previous: Option<Previous>,
}

/// A stack's template before an update.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Previous {
    pub(crate) body: String,
    pub(crate) processed: String,
    pub(crate) params: Vec<Bound>,
    pub(crate) description: Option<String>,
    pub(crate) resources: BTreeMap<String, Res>,
}

/// One resource of a stack.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct Res {
    pub(crate) ty: String,
    pub(crate) physical: Option<String>,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) updated_ms: i64,
    /// The properties it was made with, resolved, as JSON text.
    pub(crate) props: String,
    /// Its attributes (what `Fn::GetAtt` reads), as a JSON object's text.
    pub(crate) attrs: String,
    pub(crate) metadata: Option<String>,
    pub(crate) description: Option<String>,
    /// Not made locally: a skipped type with an identity of its own.
    pub(crate) skipped: bool,
}

impl Res {
    pub(crate) fn props_value(&self) -> Value {
        serde_json::from_str(&self.props).unwrap_or(Value::Null)
    }

    pub(crate) fn attrs_map(&self) -> Map<String, Value> {
        serde_json::from_str(&self.attrs).unwrap_or_default()
    }
}

/// An output of a stack.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Output {
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) description: Option<String>,
    pub(crate) export: Option<String>,
}

/// One event of a stack.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Event {
    pub(crate) stack_id: String,
    pub(crate) id: String,
    pub(crate) stack_name: String,
    pub(crate) op_id: Option<String>,
    pub(crate) logical: String,
    pub(crate) physical: String,
    pub(crate) ty: String,
    pub(crate) at_ms: i64,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) props: Option<String>,
    pub(crate) token: Option<String>,
    /// `CONFIGURATION_COMPLETE` while a resource (or the stack) waits for eventual consistency.
    pub(crate) detailed: Option<String>,
}

/// A change set.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ChangeSet {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) stack_id: String,
    pub(crate) stack_name: String,
    pub(crate) description: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) execution: String,
    /// `CREATE`, `UPDATE` or `IMPORT`.
    pub(crate) kind: String,
    pub(crate) body: String,
    pub(crate) processed: String,
    pub(crate) params: Vec<Bound>,
    pub(crate) capabilities: Vec<String>,
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) notification_arns: Vec<String>,
    /// The change records, as a JSON array's text.
    pub(crate) changes: String,
    pub(crate) include_nested: bool,
    pub(crate) on_failure: Option<String>,
    pub(crate) disable_rollback: bool,
    pub(crate) role_arn: Option<String>,
    pub(crate) parent_id: Option<String>,
    pub(crate) root_id: Option<String>,
    pub(crate) imports: Vec<String>,
}

/// What a running operation reports back to the stack's record.
#[derive(Debug, Default)]
pub(crate) struct Progress {
    pub(crate) stack_id: String,
    pub(crate) events: Vec<Draft>,
    /// Resources set (`Some`) or taken off the stack (`None`).
    pub(crate) resources: Vec<(String, Option<Res>)>,
    pub(crate) status: Option<(String, Option<String>)>,
    pub(crate) finish: Option<Finish>,
}

/// An event as the engine words it; the record adds the ids and the time.
#[derive(Clone, Debug, Default)]
pub(crate) struct Draft {
    pub(crate) logical: String,
    pub(crate) physical: String,
    pub(crate) ty: String,
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    pub(crate) props: Option<String>,
    pub(crate) detailed: Option<String>,
}

impl Draft {
    /// An event of the stack itself.
    pub(crate) fn stack(name: &str, id: &str, status: &str, reason: Option<&str>) -> Self {
        Self {
            logical: name.to_owned(),
            physical: id.to_owned(),
            ty: "AWS::CloudFormation::Stack".to_owned(),
            status: status.to_owned(),
            reason: reason.map(str::to_owned),
            props: None,
            detailed: None,
        }
    }
}

/// The end of an operation.
#[derive(Debug, Default)]
pub(crate) struct Finish {
    pub(crate) outputs: Option<Vec<Output>>,
    /// The template, processed template and parameters the stack goes back to (an update rolled back).
    pub(crate) restore: Option<(String, String, Vec<Bound>, Option<String>)>,
    pub(crate) imports: Option<Vec<String>>,
    /// The change set that was executed, and how it ended.
    pub(crate) change_set: Option<(String, String)>,
}

/// The pseudo-random id AWS puts at the end of a generated name: upper-case letters and digits.
pub(crate) fn suffix(n: usize, lower: bool) -> String {
    let s = aws_seal::alphanumeric(n).unwrap_or_else(|| "X".repeat(n));
    if lower {
        s.to_ascii_lowercase()
    } else {
        s.to_ascii_uppercase()
    }
}
