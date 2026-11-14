//! The CloudFormation calls `apply` and `export` make to a running doze-aws, over its awsQuery wire (form in, XML out), read with
//! aws-wire's XML reader.

use aws_wire::rest::xmlin::{Elem, parse};

use crate::client::Client;

/// Why a call failed.
#[derive(Debug)]
pub(crate) enum CfnError {
    /// CloudFormation refused it.
    Api { code: String, message: String },
    /// It did not get there, or the answer was not readable.
    Wire(String),
}

impl std::fmt::Display for CfnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api { code, message } => write!(f, "{message} ({code})"),
            Self::Wire(e) => f.write_str(e),
        }
    }
}

type R<T> = Result<T, CfnError>;

/// A stack.
#[derive(Debug, Default)]
pub(crate) struct Stack {
    pub(crate) status: String,
    pub(crate) reason: String,
    pub(crate) outputs: Vec<(String, String)>,
    pub(crate) params: Vec<String>,
}

/// One stack event.
#[derive(Clone, Debug, Default)]
pub(crate) struct Event {
    pub(crate) id: String,
    pub(crate) time: String,
    pub(crate) logical: String,
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) reason: String,
}

/// One change a change set would make.
#[derive(Debug, Default)]
pub(crate) struct Change {
    pub(crate) action: String,
    pub(crate) logical: String,
    pub(crate) kind: String,
    pub(crate) replacement: String,
}

/// A change set.
#[derive(Debug, Default)]
pub(crate) struct ChangeSet {
    pub(crate) status: String,
    pub(crate) reason: String,
    pub(crate) changes: Vec<Change>,
}

/// The capabilities `apply` acknowledges: it acts on a local account, where the acknowledgement guards nothing.
const CAPABILITIES: [&str; 3] = [
    "CAPABILITY_IAM",
    "CAPABILITY_NAMED_IAM",
    "CAPABILITY_AUTO_EXPAND",
];

/// CloudFormation on a running doze-aws.
#[derive(Debug)]
pub(crate) struct Cfn {
    client: Client,
}

/// Form fields.
type Form = Vec<(String, String)>;

fn field(k: impl Into<String>, v: impl Into<String>) -> (String, String) {
    (k.into(), v.into())
}

impl Cfn {
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }

    fn call(&self, action: &str, mut form: Form) -> R<Elem> {
        form.insert(0, field("Action", action));
        form.insert(1, field("Version", "2010-05-15"));
        let reply = self
            .client
            .query("cloudformation", &form)
            .map_err(CfnError::Wire)?;
        let root = parse(&reply.body).map_err(|_| {
            CfnError::Wire(format!(
                "CloudFormation answered {} with something that is not XML: {}",
                reply.status,
                String::from_utf8_lossy(&reply.body)
            ))
        })?;
        if reply.status >= 300 {
            let err = child(&root, "Error");
            return Err(CfnError::Api {
                code: err
                    .and_then(|e| text(e, "Code"))
                    .unwrap_or("Unknown")
                    .to_owned(),
                message: err
                    .and_then(|e| text(e, "Message"))
                    .unwrap_or_default()
                    .to_owned(),
            });
        }
        let result = format!("{action}Result");
        Ok(child(&root, &result).cloned().unwrap_or_default())
    }

    /// The stack called `name`, if there is one.
    pub(crate) fn stack(&self, name: &str) -> R<Option<Stack>> {
        let out = match self.call("DescribeStacks", vec![field("StackName", name)]) {
            Err(CfnError::Api { message, .. }) if message.contains("does not exist") => {
                return Ok(None);
            }
            other => other?,
        };
        let Some(s) = members(&out, "Stacks").next() else {
            return Ok(None);
        };
        Ok(Some(Stack {
            status: text(s, "StackStatus").unwrap_or_default().to_owned(),
            reason: text(s, "StackStatusReason").unwrap_or_default().to_owned(),
            outputs: members(s, "Outputs")
                .map(|o| (own(o, "OutputKey"), own(o, "OutputValue")))
                .collect(),
            params: members(s, "Parameters")
                .map(|p| own(p, "ParameterKey"))
                .collect(),
        }))
    }

    /// The stack's latest events, newest first.
    pub(crate) fn events(&self, name: &str) -> R<Vec<Event>> {
        let out = self.call("DescribeStackEvents", vec![field("StackName", name)])?;
        Ok(members(&out, "StackEvents")
            .map(|e| Event {
                id: own(e, "EventId"),
                time: own(e, "Timestamp"),
                logical: own(e, "LogicalResourceId"),
                kind: own(e, "ResourceType"),
                status: own(e, "ResourceStatus"),
                reason: own(e, "ResourceStatusReason"),
            })
            .collect())
    }

    /// Creates the stack.
    pub(crate) fn create_stack(
        &self,
        name: &str,
        body: &str,
        params: &[(String, Option<String>)],
    ) -> R<()> {
        let mut form = vec![field("StackName", name), field("TemplateBody", body)];
        add_params(&mut form, params);
        self.call("CreateStack", form).map(|_| ())
    }

    /// Makes an `UPDATE` change set and answers its id.
    pub(crate) fn create_change_set(
        &self,
        name: &str,
        set: &str,
        body: &str,
        params: &[(String, Option<String>)],
    ) -> R<String> {
        let mut form = vec![
            field("StackName", name),
            field("ChangeSetName", set),
            field("ChangeSetType", "UPDATE"),
            field("TemplateBody", body),
        ];
        add_params(&mut form, params);
        let out = self.call("CreateChangeSet", form)?;
        Ok(own(&out, "Id"))
    }

    /// The change set.
    pub(crate) fn change_set(&self, id: &str) -> R<ChangeSet> {
        let out = self.call("DescribeChangeSet", vec![field("ChangeSetName", id)])?;
        Ok(ChangeSet {
            status: own(&out, "Status"),
            reason: own(&out, "StatusReason"),
            changes: members(&out, "Changes")
                .filter_map(|c| child(c, "ResourceChange"))
                .map(|r| Change {
                    action: own(r, "Action"),
                    logical: own(r, "LogicalResourceId"),
                    kind: own(r, "ResourceType"),
                    replacement: own(r, "Replacement"),
                })
                .collect(),
        })
    }

    pub(crate) fn execute_change_set(&self, id: &str) -> R<()> {
        self.call("ExecuteChangeSet", vec![field("ChangeSetName", id)])
            .map(|_| ())
    }

    pub(crate) fn delete_change_set(&self, id: &str) -> R<()> {
        self.call("DeleteChangeSet", vec![field("ChangeSetName", id)])
            .map(|_| ())
    }

    pub(crate) fn delete_stack(&self, name: &str) -> R<()> {
        self.call("DeleteStack", vec![field("StackName", name)])
            .map(|_| ())
    }

    /// The parameters `body` declares.
    pub(crate) fn declared_params(&self, body: &str) -> R<Vec<String>> {
        let out = self.call("GetTemplateSummary", vec![field("TemplateBody", body)])?;
        Ok(members(&out, "Parameters")
            .map(|p| own(p, "ParameterKey"))
            .collect())
    }

    /// The stack's template, as written or after its transforms.
    pub(crate) fn template(&self, name: &str, processed: bool) -> R<String> {
        let stage = if processed { "Processed" } else { "Original" };
        let out = self.call(
            "GetTemplate",
            vec![field("StackName", name), field("TemplateStage", stage)],
        )?;
        Ok(own(&out, "TemplateBody"))
    }
}

/// Parameters (a `None` value keeps the stack's current one) and the capabilities.
fn add_params(form: &mut Form, params: &[(String, Option<String>)]) {
    for (i, (key, value)) in params.iter().enumerate() {
        let n = i + 1;
        form.push(field(
            format!("Parameters.member.{n}.ParameterKey"),
            key.clone(),
        ));
        match value {
            Some(v) => form.push(field(
                format!("Parameters.member.{n}.ParameterValue"),
                v.clone(),
            )),
            None => form.push(field(
                format!("Parameters.member.{n}.UsePreviousValue"),
                "true",
            )),
        }
    }
    for (i, cap) in CAPABILITIES.iter().enumerate() {
        form.push(field(format!("Capabilities.member.{}", i + 1), *cap));
    }
}

fn child<'a>(e: &'a Elem, name: &str) -> Option<&'a Elem> {
    e.children.iter().find(|c| c.name == name)
}

fn text<'a>(e: &'a Elem, name: &str) -> Option<&'a str> {
    child(e, name).map(|c| c.text.as_str())
}

fn own(e: &Elem, name: &str) -> String {
    text(e, name).unwrap_or_default().to_owned()
}

/// The `member`s of list `name` inside `e`.
fn members<'a>(e: &'a Elem, name: &str) -> impl Iterator<Item = &'a Elem> {
    child(e, name)
        .into_iter()
        .flat_map(|l| l.children.iter().filter(|c| c.name == "member"))
}
