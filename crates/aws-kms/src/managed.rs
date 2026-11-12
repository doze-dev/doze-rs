//! The keys AWS makes for its services: `alias/aws/ssm` for SSM `SecureString`s, `alias/aws/secretsmanager` for secrets. Such a key
//! does not exist until the service first asks for it, and only that service may ask: a request that came in over the wire for
//! `alias/aws/ssm` finds nothing, as it would in an account whose SSM has never used it.

use aws_wire::ApiError;
use aws_wire::service::Call;

use crate::core::Core;
use crate::keys::{default_policy, no_randomness};
use crate::model::{Alias, Key, State};
use crate::spec::{Spec, Usage};

/// The service that owns each AWS-managed alias, and the words AWS gives the key.
const MANAGED: [(&str, &str, &str); 2] = [
    (
        "alias/aws/ssm",
        "ssm",
        "Default master key that protects my SSM parameters when no other key is defined",
    ),
    (
        "alias/aws/secretsmanager",
        "secretsmanager",
        "Default master key that protects my Secrets Manager data when no other key is defined",
    ),
];

impl Core {
    /// Makes the AWS-managed key `given` names, when it is one, the service named in the call is its owner, and it is not yet there.
    pub(crate) fn ensure_managed(&mut self, call: &Call, given: &str) -> Result<(), ApiError> {
        let name = given
            .strip_prefix("arn:")
            .and_then(|a| a.rsplit_once(':').map(|(_, t)| t))
            .unwrap_or(given);
        let Some((alias, _, description)) = MANAGED.iter().find(|(alias, owner, _)| {
            *alias == name && call.caller.service.as_deref() == Some(*owner)
        }) else {
            return Ok(());
        };
        if self.aliases.get(alias).is_some() {
            return Ok(());
        }
        let id = aws_seal::uuid_v4().ok_or_else(no_randomness)?;
        let now = self.now_ms();
        let key = Key {
            arn: crate::find::key_arn(call, &id),
            id: id.clone(),
            spec: Spec::Symmetric,
            usage: Usage::EncryptDecrypt,
            description: (*description).to_owned(),
            state: State::Enabled,
            created_ms: now,
            purge_ms: None,
            window_days: None,
            materials: vec![self.new_material(Spec::Symmetric, now)?],
            policy: Some(default_policy(&call.caller.account_id)),
            tags: std::collections::BTreeMap::new(),
            rotation_enabled: false,
            rotation_days: 365,
            next_rotation_ms: None,
            multi_region: false,
            grants: Vec::new(),
            external: false,
            aws_managed: true,
            import: None,
            expiration_model: None,
            valid_to_ms: None,
        };
        self.keys.insert(id.clone(), key);
        self.aliases.insert(
            (*alias).to_owned(),
            Alias {
                name: (*alias).to_owned(),
                key_id: id,
                created_ms: now,
                updated_ms: now,
            },
        );
        Ok(())
    }
}
