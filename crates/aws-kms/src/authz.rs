//! What a request asks to do, for the authorizer: the IAM action (`kms:Decrypt`), the ARN of each key it names, the key's policy, and
//! the encryption context it carries. A key insists on being named in its own policy before an identity policy counts, so every key
//! need is gated.

use aws_model_kms::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;
use crate::crypto;
use crate::find::{alias_arn, key_arn};

impl Core {
    /// Every thing `call` asks to do.
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        use Operation as O;
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |key: &str| body.get(key).and_then(Value::as_str);
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let everything =
            |action: &str| Need::new(action, format!("arn:aws:kms:{region}:{account}:*"));
        let name = format!("kms:{op:?}");
        match op {
            O::CreateKey
            | O::ListKeys
            | O::ListAliases
            | O::GenerateRandom
            | O::ListRetirableGrants
            | O::RetireGrant => {
                vec![everything(&name)]
            }
            O::ReEncrypt => vec![
                self.on_key(
                    call,
                    "kms:ReEncryptFrom",
                    text("SourceKeyId")
                        .map(str::to_owned)
                        .or_else(|| blob_key(text("CiphertextBlob"))),
                    &body,
                ),
                self.on_key(
                    call,
                    "kms:ReEncryptTo",
                    text("DestinationKeyId").map(str::to_owned),
                    &body,
                ),
            ],
            O::Decrypt => vec![
                self.on_key(
                    call,
                    &name,
                    text("KeyId")
                        .map(str::to_owned)
                        .or_else(|| blob_key(text("CiphertextBlob"))),
                    &body,
                ),
            ],
            O::CreateAlias | O::UpdateAlias => {
                let mut needs = Vec::new();
                if let Some(alias) = text("AliasName") {
                    needs.push(Need::new(&name, alias_arn(call, alias)));
                }
                needs.push(self.on_key(call, &name, text("TargetKeyId").map(str::to_owned), &body));
                needs
            }
            O::DeleteAlias => text("AliasName")
                .map(|alias| Need::new(&name, alias_arn(call, alias)))
                .into_iter()
                .collect(),
            _ => vec![self.on_key(call, &name, text("KeyId").map(str::to_owned), &body)],
        }
    }

    /// An action on the key `given` names, with its policy, and the encryption context the request carries.
    fn on_key(&self, call: &Call, action: &str, given: Option<String>, body: &Value) -> Need {
        let located = given.as_deref().and_then(|g| self.locate(call, g).ok());
        let mut need = match (&located, &given) {
            (Some(id), _) => Need::new(action, key_arn(call, id)),
            (None, Some(g)) if g.starts_with("arn:") => Need::new(action, g.clone()),
            (None, Some(g)) => Need::new(action, key_arn(call, g.trim_start_matches("key/"))),
            (None, None) => Need::new(action, key_arn(call, "*")),
        }
        .gated();
        need.policy = located
            .as_deref()
            .and_then(|id| self.keys.get(id))
            .and_then(|k| k.policy.clone());
        if let Some(context) = body.get("EncryptionContext").and_then(Value::as_object) {
            let keys: Vec<String> = context.keys().cloned().collect();
            need = need.with("kms:EncryptionContextKeys", keys);
            for (k, v) in context {
                if let Some(v) = v.as_str() {
                    need = need.with(&format!("kms:EncryptionContext:{k}"), vec![v.to_owned()]);
                }
            }
        }
        if let Some(service) = &call.caller.service {
            need = need.with(
                "kms:ViaService",
                vec![format!("{service}.{}.amazonaws.com", call.caller.region)],
            );
        }
        need
    }
}

/// The id of the key a ciphertext blob (base64) was made under.
fn blob_key(blob: Option<&str>) -> Option<String> {
    let bytes = aws_core::base64::decode(blob?)?;
    crypto::parse(&bytes).map(|b| b.key_id.to_owned())
}
