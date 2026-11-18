//! What a request asks to do, for the authorizer: the IAM action (`ssm:GetParameter`), the ARN of each parameter, document or
//! service setting it names, and the resource policy on it when it has one (`PutResourcePolicy`). The authorizer reads that policy as
//! the resource-based layer, as it reads a queue's or a secret's: it can grant a principal what its identity policies do not, and its
//! explicit deny refuses whatever they allow.

use aws_model_ssm::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;
use crate::resource_policy::{document_key, parameter_key};

impl Core {
    /// Every thing `call` asks to do.
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("ssm:{op:?}");
        let on = |name: &str| self.parameter_need(&action, call, name);
        let document = |name: &str| self.document_need(&action, call, name);
        let text = |key: &str| body.get(key).and_then(Value::as_str);
        let one = |key: &str| text(key).map(on);
        match op {
            Operation::GetParameters | Operation::DeleteParameters => body
                .get("Names")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(on)
                .collect(),
            Operation::GetParametersByPath => one("Path").into_iter().collect(),
            Operation::AddTagsToResource
            | Operation::RemoveTagsFromResource
            | Operation::ListTagsForResource => match text("ResourceType") {
                Some("Document") => text("ResourceId").map(document).into_iter().collect(),
                _ => one("ResourceId").into_iter().collect(),
            },
            Operation::PutResourcePolicy
            | Operation::GetResourcePolicies
            | Operation::DeleteResourcePolicy => text("ResourceArn")
                .map(|arn| match arn.split_once(":document/") {
                    Some(_) => document(arn),
                    None if arn.contains(":parameter/") => on(arn),
                    None => Need::new(action.clone(), arn),
                })
                .into_iter()
                .collect(),
            Operation::DescribeParameters | Operation::ListDocuments => vec![Need::new(
                action.clone(),
                format!("arn:aws:ssm:{region}:{account}:*"),
            )],
            Operation::GetServiceSetting
            | Operation::UpdateServiceSetting
            | Operation::ResetServiceSetting => text("SettingId")
                .map(|id| setting(&action, region, account, id))
                .into_iter()
                .collect(),
            Operation::CreateDocument
            | Operation::GetDocument
            | Operation::DescribeDocument
            | Operation::UpdateDocument
            | Operation::DeleteDocument
            | Operation::ListDocumentVersions
            | Operation::UpdateDocumentDefaultVersion
            | Operation::DescribeDocumentPermission
            | Operation::ModifyDocumentPermission
            | Operation::UpdateDocumentMetadata
            | Operation::ListDocumentMetadataHistory => {
                text("Name").map(document).into_iter().collect()
            }
            _ => one("Name").into_iter().collect(),
        }
    }

    /// `action` on the parameter `name` (a name, a selector such as `name:3`, or an ARN), with its policy.
    fn parameter_need(&self, action: &str, call: &Call, name: &str) -> Need {
        let selected = crate::name::selector(name).name;
        // An ARN names its own account; a bare name is the caller's. A public parameter belongs to no account.
        let owner = match name.strip_prefix("arn:") {
            Some(rest) => rest.split(':').nth(3).unwrap_or(&call.caller.account_id),
            None if crate::public::is_public(selected) => "",
            None => call.caller.account_id.as_str(),
        };
        Need::new(
            action,
            format!(
                "arn:aws:ssm:{}:{owner}:parameter/{}",
                call.caller.region,
                selected.trim_start_matches('/')
            ),
        )
        .with_policy(self.policy_of(&parameter_key(selected)))
    }

    /// `action` on the document `name` (or its ARN), with its policy.
    fn document_need(&self, action: &str, call: &Call, name: &str) -> Need {
        let bare = crate::documents::bare(name);
        Need::new(
            action,
            format!(
                "arn:aws:ssm:{}:{}:document/{bare}",
                call.caller.region, call.caller.account_id
            ),
        )
        .with_policy(self.policy_of(&document_key(bare)))
    }
}

/// A service setting, named by its id or its ARN.
fn setting(action: &str, region: &str, account: &str, id: &str) -> Need {
    let id = id.split_once(":servicesetting").map_or(id, |(_, id)| id);
    Need::new(
        action,
        format!("arn:aws:ssm:{region}:{account}:servicesetting{id}"),
    )
}
