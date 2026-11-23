//! What the Secrets Manager pages of the console may ask of the service without a secret bound: the wizard's `CreateSecret` (and the
//! rotation, resource policy and tags it adds after it), and what a secret's own page reads and writes. A page names its secret in
//! the input; each call is the service's own, made as the caller `console`. Also how the list's search tokens become `ListSecrets`
//! filters.

use serde_json::{Value, json};

use super::{Op, ops::free};

/// The secrets' pages.
pub(super) const OPS: &[Op] = &[
    free("CreateSecret"),
    free("UpdateSecret"),
    free("DescribeSecret"),
    free("GetSecretValue"),
    free("PutSecretValue"),
    free("ListSecretVersionIds"),
    free("RotateSecret"),
    free("CancelRotateSecret"),
    free("GetResourcePolicy"),
    free("PutResourcePolicy"),
    free("DeleteResourcePolicy"),
    free("DeleteSecret"),
    free("RestoreSecret"),
    free("TagResource"),
    free("UntagResource"),
];

/// The keys of `ListSecrets` filters a search token may carry.
const KEYS: [&str; 5] = ["name", "description", "tag-key", "tag-value", "all"];

/// The search tokens of the list (`name:larder/prod/|tag-key:app`, joined by `|`) as the `Filters` of `ListSecrets`. A token with a
/// key the service does not have, or no text, is left out.
#[must_use]
pub(super) fn filters(tokens: &str) -> Value {
    let list: Vec<Value> = tokens
        .split('|')
        .filter_map(|t| t.split_once(':'))
        .filter(|(k, v)| KEYS.contains(k) && !v.is_empty())
        .map(|(k, v)| json!({"Key": k, "Values": [v]}))
        .collect();
    if list.is_empty() {
        json!({})
    } else {
        json!({"Filters": list})
    }
}

/// Whether the list shows secrets scheduled for deletion: `yes` does.
#[must_use]
pub(super) fn planned(value: &str) -> Value {
    if value == "yes" {
        json!({"IncludePlannedDeletion": true})
    } else {
        json!({})
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{filters, planned};

    #[test]
    fn tokens_become_filters_and_what_the_service_does_not_have_is_dropped() {
        assert_eq!(
            filters("name:larder/prod/|tag-key:app|nope:x|description:"),
            json!({"Filters": [
                {"Key": "name", "Values": ["larder/prod/"]},
                {"Key": "tag-key", "Values": ["app"]},
            ]})
        );
        assert_eq!(filters(""), json!({}));
        assert_eq!(
            filters("all:stripe"),
            json!({"Filters": [{"Key": "all", "Values": ["stripe"]}]})
        );
    }

    #[test]
    fn scheduled_secrets_are_listed_only_when_asked_for() {
        assert_eq!(planned("yes"), json!({"IncludePlannedDeletion": true}));
        assert_eq!(planned("no"), json!({}));
    }
}
