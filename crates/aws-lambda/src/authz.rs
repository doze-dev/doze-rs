//! What a request asks to do, for the authorizer: the IAM action (`lambda:<Operation>`), the ARN acted on, and the function's own
//! policy as the resource layer. Creating or re-pointing a function at a role also asks for `iam:PassRole` on the role, to Lambda.

use aws_model_lambda::Operation;
use aws_model_lambda::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;
use serde_json::{Map, Value};

use crate::arn;
use crate::core::Core;
use crate::req::Req;

/// The text members `needs` reads.
const TEXT: &[&str] = &[
    "FunctionName",
    "Qualifier",
    "Role",
    "LayerName",
    "Arn",
    "UUID",
    "Resource",
    "ResourceArn",
    "DurableExecutionArn",
    "CallbackId",
    "CodeSigningConfigArn",
];

/// The request as `needs` reads it: only the members it looks at, read from the decoded input. Parsing the whole body again would
/// read every byte of an `Invoke` payload (up to 6 MB) for a function name.
fn slim(call: &Call) -> Req {
    let input = In::of(&TABLES, call);
    let mut body = Map::new();
    for key in TEXT {
        if let Some(text) = input.text(key) {
            body.insert((*key).to_owned(), Value::String(text.to_owned()));
        }
    }
    if let Some(n) = input.get("VersionNumber").and_then(|v| v.as_i64()) {
        body.insert("VersionNumber".to_owned(), Value::from(n));
    }
    // Only whether tags were given is asked.
    if input.get("Tags").is_some() {
        body.insert("Tags".to_owned(), Value::Object(Map::new()));
    }
    Req {
        body: Value::Object(body),
        caller: call.caller.clone(),
        payload: None,
    }
}

impl Core {
    /// Every thing `call` asks to do. Empty for a request that is not understood (it is refused on its own terms).
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let r = slim(call);
        let action = format!("lambda:{op:?}");
        let mut needs = self.resource_need(op, &r, &action);
        let role = match op {
            Operation::CreateFunction | Operation::UpdateFunctionConfiguration => r.opt("Role"),
            _ => None,
        };
        if let Some(role) = role {
            needs.push(Need::new("iam:PassRole", role).with(
                "iam:PassedToService",
                vec!["lambda.amazonaws.com".to_owned()],
            ));
        }
        if op == Operation::CreateFunction && r.value("Tags").is_some() {
            let f = r
                .function()
                .ok()
                .map(|f| arn::function(r.region(), r.account(), &f.name, None));
            needs.extend(f.map(|a| Need::new("lambda:TagResource", a)));
        }
        needs
    }

    fn resource_need(&self, op: Operation, r: &Req, action: &str) -> Vec<Need> {
        match op {
            Operation::ListFunctions
            | Operation::ListLayers
            | Operation::GetAccountSettings
            | Operation::ListEventSourceMappings
            | Operation::CreateEventSourceMapping
            | Operation::ListCodeSigningConfigs
            | Operation::CreateCodeSigningConfig => vec![Need::new(action, "*")],
            Operation::PublishLayerVersion
            | Operation::ListLayerVersions
            | Operation::GetLayerVersion
            | Operation::DeleteLayerVersion
            | Operation::AddLayerVersionPermission
            | Operation::RemoveLayerVersionPermission
            | Operation::GetLayerVersionPolicy => {
                let name = r.text("LayerName");
                let version = r.int("VersionNumber").and_then(|v| u32::try_from(v).ok());
                let name = name.rsplit(':').next().unwrap_or(name);
                vec![Need::new(
                    action,
                    arn::layer(r.region(), r.account(), name, version),
                )]
            }
            Operation::GetLayerVersionByArn => {
                vec![Need::new("lambda:GetLayerVersion", r.text("Arn"))]
            }
            Operation::GetEventSourceMapping
            | Operation::UpdateEventSourceMapping
            | Operation::DeleteEventSourceMapping => {
                vec![Need::new(
                    action,
                    arn::mapping(r.region(), r.account(), r.text("UUID")),
                )]
            }
            Operation::TagResource | Operation::UntagResource | Operation::ListTags => {
                vec![Need::new(action, r.text("Resource"))]
            }
            Operation::PutResourcePolicy
            | Operation::GetResourcePolicy
            | Operation::DeleteResourcePolicy => {
                vec![Need::new(action, r.text("ResourceArn"))]
            }
            // Memory: the resource of these actions is taken to be the execution's ARN (the callback id names it).
            Operation::CheckpointDurableExecution
            | Operation::GetDurableExecution
            | Operation::GetDurableExecutionHistory
            | Operation::GetDurableExecutionState
            | Operation::StopDurableExecution => {
                vec![Need::new(action, r.text("DurableExecutionArn"))]
            }
            Operation::SendDurableExecutionCallbackSuccess
            | Operation::SendDurableExecutionCallbackFailure
            | Operation::SendDurableExecutionCallbackHeartbeat => {
                let arn = crate::durable::callback_target(r.text("CallbackId"))
                    .map_or_else(|| "*".to_owned(), |(arn, _)| arn);
                vec![Need::new(action, arn)]
            }
            Operation::GetCodeSigningConfig
            | Operation::UpdateCodeSigningConfig
            | Operation::DeleteCodeSigningConfig
            | Operation::ListFunctionsByCodeSigningConfig => {
                vec![Need::new(action, r.text("CodeSigningConfigArn"))]
            }
            _ => self.function_need(r, action),
        }
    }

    /// A need on the function a request names, with its policy.
    fn function_need(&self, r: &Req, action: &str) -> Vec<Need> {
        let Ok(f) = r.function() else {
            return vec![Need::new(action, "*")];
        };
        let policy = self
            .functions
            .get(&f.name)
            .and_then(|function| {
                function
                    .policies
                    .get(&f.qualifier.clone().unwrap_or_default())
            })
            .cloned();
        let target = arn::function(r.region(), r.account(), &f.name, f.qualifier.as_deref());
        vec![Need::new(action, target).with_policy(policy)]
    }
}
