//! Finding a secret by the id a request gives: its name, its full ARN, or its ARN without the six-character suffix.

use aws_wire::ApiError;

use crate::core::{Core, marked_for_deletion, not_found};
use crate::model::Secret;

impl Core {
    /// The name of the secret `id` stands for: a name, a full ARN, or an ARN without its suffix of a dash and six characters.
    pub(crate) fn locate(&self, id: &str) -> Result<String, ApiError> {
        let Some(rest) = id.strip_prefix("arn:") else {
            return self
                .secrets
                .get(id)
                .map(|s| s.name.clone())
                .ok_or_else(not_found);
        };
        let (_, tail) = rest.split_once(":secret:").ok_or_else(not_found)?;
        let full = split_suffix(tail)
            .and_then(|stem| self.secrets.get(stem))
            .filter(|s| s.arn == id);
        full.or_else(|| self.secrets.get(tail))
            .map(|s| s.name.clone())
            .ok_or_else(not_found)
    }

    /// The secret `id` stands for.
    pub(crate) fn secret(&self, id: &str) -> Result<&Secret, ApiError> {
        let name = self.locate(id)?;
        self.secrets.get(&name).ok_or_else(not_found)
    }

    /// The name of a secret that is not marked for deletion.
    pub(crate) fn locate_live(&self, id: &str) -> Result<String, ApiError> {
        let name = self.locate(id)?;
        match self.secrets.get(&name) {
            Some(s) if s.deleted_ms.is_some() => Err(marked_for_deletion()),
            Some(_) => Ok(name),
            None => Err(not_found()),
        }
    }
}

/// The name part of `name-AbC123`.
fn split_suffix(tail: &str) -> Option<&str> {
    let cut = tail.len().checked_sub(7)?;
    (tail.get(cut..cut + 1) == Some("-"))
        .then(|| tail.get(..cut))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::split_suffix;

    #[test]
    fn the_suffix_of_an_arn_is_a_dash_and_six_characters() {
        assert_eq!(split_suffix("db/pass-AbC123"), Some("db/pass"));
        assert_eq!(split_suffix("short"), None);
        assert_eq!(split_suffix("nodashhere1234"), None);
    }
}
