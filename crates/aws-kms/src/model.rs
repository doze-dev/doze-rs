//! What KMS holds: keys with their material, and aliases.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::spec::{Spec, Usage};

pub(crate) const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum State {
    Enabled,
    Disabled,
    PendingDeletion,
    /// An external key (its material is imported) that has no material: new, deleted, or expired.
    PendingImport,
}

impl State {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
            Self::PendingDeletion => "PendingDeletion",
            Self::PendingImport => "PendingImport",
        }
    }
}

/// One generation of a key's material. A key that has been rotated keeps the old ones, so what they sealed still opens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Material {
    /// The SHA-256 of the material, as hex: what `CurrentKeyMaterialId` shows.
    pub(crate) id: String,
    /// The material, sealed under the service's own key.
    pub(crate) sealed: Vec<u8>,
    pub(crate) created_ms: i64,
    /// How the generation came to be replaced: `ON_DEMAND` or `AUTOMATIC`; none while it is current.
    pub(crate) retired_by: Option<String>,
    pub(crate) retired_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Key {
    pub(crate) id: String,
    pub(crate) arn: String,
    pub(crate) spec: Spec,
    pub(crate) usage: Usage,
    pub(crate) description: String,
    pub(crate) state: State,
    pub(crate) created_ms: i64,
    /// When a key scheduled for deletion goes, and the window it was given.
    pub(crate) purge_ms: Option<i64>,
    pub(crate) window_days: Option<i64>,
    /// The generations of material, oldest first; the last is current.
    pub(crate) materials: Vec<Material>,
    pub(crate) policy: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) rotation_enabled: bool,
    pub(crate) rotation_days: i64,
    pub(crate) next_rotation_ms: Option<i64>,
    pub(crate) multi_region: bool,
    pub(crate) grants: Vec<Grant>,
    /// Whether the key's material is imported (`Origin` is `EXTERNAL`) and not made here.
    pub(crate) external: bool,
    /// Whether AWS made and manages the key for a service (`alias/aws/ssm`) and not the customer.
    pub(crate) aws_managed: bool,
    /// The wrapping key pair handed out by `GetParametersForImport`, until the material is imported.
    pub(crate) import: Option<Import>,
    pub(crate) expiration_model: Option<String>,
    /// When imported material expires, if it does.
    pub(crate) valid_to_ms: Option<i64>,
}

/// What `GetParametersForImport` handed out: a token, and the RSA key that wraps the material.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Import {
    pub(crate) token: Vec<u8>,
    /// The wrapping private key (PKCS #8), sealed.
    pub(crate) wrapping: Vec<u8>,
    pub(crate) algorithm: String,
    pub(crate) valid_to_ms: i64,
}

/// A grant: permission for a principal to use a key for some operations. Kept and listed; it is not evaluated until the IAM stage can.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Grant {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) name: Option<String>,
    pub(crate) grantee: String,
    pub(crate) retiring: Option<String>,
    pub(crate) operations: Vec<String>,
    pub(crate) context_subset: BTreeMap<String, String>,
    pub(crate) context_equals: BTreeMap<String, String>,
    pub(crate) created_ms: i64,
}

impl Key {
    pub(crate) fn current(&self) -> Option<&Material> {
        self.materials.last()
    }

    pub(crate) fn material(&self, id_hex: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.id == id_hex)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Alias {
    /// `alias/name`.
    pub(crate) name: String,
    pub(crate) key_id: String,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
}
