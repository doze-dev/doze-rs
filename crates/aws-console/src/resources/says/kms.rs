//! KMS: how a key's page speaks. The header says the key's state (`Enabled`, `Pending deletion`) and what kind of key it is
//! (`Symmetric`, `Asymmetric`, `HMAC`), as the console's boards word them. The sentence needs the key's rotations and its aliases,
//! which the app reads, so it says none here.

use super::super::{Page, Read};
use super::tone;
use crate::model::{Badge, Tone};

/// `PendingDeletion` as `Pending deletion`: the state as people say it.
fn state_words(state: &str) -> String {
    match state {
        "PendingDeletion" => "Pending deletion".to_owned(),
        "PendingImport" => "Pending import".to_owned(),
        "PendingReplicaDeletion" => "Pending replica deletion".to_owned(),
        other => other.to_owned(),
    }
}

/// What kind of key a spec is.
#[must_use]
pub fn kind_of(spec: &str) -> &'static str {
    if spec.starts_with("HMAC") {
        "HMAC"
    } else if spec.starts_with("SYMMETRIC") {
        "Symmetric"
    } else if spec.starts_with("RSA") || spec.starts_with("ECC") || spec == "SM2" {
        "Asymmetric"
    } else {
        "Key"
    }
}

fn badges(r: &Read<'_>) -> Vec<Badge> {
    let mut out = Vec::new();
    if let Some(state) = r.text("KeyMetadata.KeyState") {
        out.push(Badge::new(state_words(&state), tone(&state)));
    }
    if let Some(spec) = r.text("KeyMetadata.KeySpec") {
        out.push(Badge::new(kind_of(&spec), Tone::Neutral));
    }
    out
}

/// A key's header: its state and its kind.
pub const KEY: Page = Page {
    sentence: None,
    badges: Some(badges),
    verbs: &[],
    summary: None,
};

#[cfg(test)]
mod tests {
    use super::{kind_of, state_words};

    #[test]
    fn a_spec_is_symmetric_asymmetric_or_hmac_and_a_state_is_said_in_words() {
        assert_eq!(kind_of("SYMMETRIC_DEFAULT"), "Symmetric");
        assert_eq!(kind_of("RSA_2048"), "Asymmetric");
        assert_eq!(kind_of("ECC_NIST_P256"), "Asymmetric");
        assert_eq!(kind_of("HMAC_256"), "HMAC");
        assert_eq!(state_words("PendingDeletion"), "Pending deletion");
        assert_eq!(state_words("Enabled"), "Enabled");
    }
}
