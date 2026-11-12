//! What a request asks to do, for the authorizer: the IAM action (`secretsmanager:GetSecretValue`), the ARN of each secret it names,
//! and the resource policy on each.

use aws_model_secrets_manager::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;

impl Core {
    /// Every thing `call` asks to do.
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("secretsmanager:{op:?}");
        let anything = || {
            vec![Need::new(
                action.clone(),
                format!("arn:aws:secretsmanager:{region}:{account}:*"),
            )]
        };
        // A secret by the id a request gives: its ARN and policy if it exists, else the id as the ARN's name.
        let on = |id: &str| match self.secret(id) {
            Ok(s) => Need::new(action.clone(), s.arn.clone()).with_policy(s.policy.clone()),
            Err(_) if id.starts_with("arn:") => Need::new(action.clone(), id),
            Err(_) => Need::new(
                action.clone(),
                format!("arn:aws:secretsmanager:{region}:{account}:secret:{id}"),
            ),
        };
        let text = |key: &str| body.get(key).and_then(Value::as_str);
        match op {
            Operation::ListSecrets | Operation::GetRandomPassword => anything(),
            Operation::CreateSecret => text("Name").map(on).into_iter().collect(),
            Operation::BatchGetSecretValue => {
                match body.get("SecretIdList").and_then(Value::as_array) {
                    Some(ids) => ids.iter().filter_map(Value::as_str).map(on).collect(),
                    None => anything(),
                }
            }
            Operation::ValidateResourcePolicy if text("SecretId").is_none() => anything(),
            _ => text("SecretId").map(on).into_iter().collect(),
        }
    }
}
