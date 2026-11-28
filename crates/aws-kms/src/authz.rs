//! What a request asks to do, for the authorizer: the IAM action (`kms:Decrypt`), the ARN of each key it names, the key's policy, and
//! the encryption context it carries. A key insists on being named in its own policy before an identity policy counts, so every key
//! need is gated.

use aws_model_kms::Operation;
use aws_model_kms::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

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
        let body = In::of(&TABLES, call);
        let text = |key: &str| body.get(key).and_then(|v| v.as_str());
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
                    text("SourceKeyId").map(str::to_owned).or_else(|| {
                        blob_key(body.get("CiphertextBlob").and_then(|v| v.as_bytes()))
                    }),
                    &body,
                ),
                self.on_key(
                    call,
                    "kms:ReEncryptTo",
                    text("DestinationKeyId").map(str::to_owned),
                    &body,
                ),
            ],
            O::Decrypt => {
                vec![self.on_key(
                    call,
                    &name,
                    text("KeyId").map(str::to_owned).or_else(|| {
                        blob_key(body.get("CiphertextBlob").and_then(|v| v.as_bytes()))
                    }),
                    &body,
                )]
            }
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
    fn on_key(&self, call: &Call, action: &str, given: Option<String>, body: &In<'_>) -> Need {
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
        if let Some(context) = body.get("EncryptionContext") {
            // In the order of their names, as the parsed body listed them.
            let mut pairs: Vec<(&str, In<'_>)> = context.pairs().collect();
            pairs.sort_by_key(|(k, _)| *k);
            let keys: Vec<String> = pairs.iter().map(|(k, _)| (*k).to_owned()).collect();
            need = need.with("kms:EncryptionContextKeys", keys);
            for (k, v) in pairs {
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

/// The id of the key a ciphertext blob was made under.
fn blob_key(blob: Option<&[u8]>) -> Option<String> {
    crypto::parse(blob?).map(|b| b.key_id.to_owned())
}
