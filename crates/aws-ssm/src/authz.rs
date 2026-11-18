//! What a request asks to do, for the authorizer: the IAM action (`ssm:GetParameter`) and the ARN of each parameter, document or
//! service setting it names.

use aws_model_ssm::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("ssm:{op:?}");
    let on = |name: &str| {
        // A version or label selector (`name:3`) is not part of the ARN.
        let bare = name.split_once(':').map_or(name, |(n, _)| n);
        // A public parameter belongs to no account.
        let owner = if crate::public::is_public(bare) {
            ""
        } else {
            account.as_str()
        };
        Need::new(
            action.clone(),
            format!(
                "arn:aws:ssm:{region}:{owner}:parameter/{}",
                bare.trim_start_matches('/')
            ),
        )
    };
    let document = |name: &str| {
        Need::new(
            action.clone(),
            format!(
                "arn:aws:ssm:{region}:{account}:document/{}",
                crate::documents::bare(name)
            ),
        )
    };
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
        | Operation::UpdateDocumentMetadata => text("Name").map(document).into_iter().collect(),
        _ => one("Name").into_iter().collect(),
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
