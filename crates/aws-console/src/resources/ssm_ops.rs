//! What the Parameter Store pages of the console may ask of SSM without a parameter bound: the form that makes one (`PutParameter`,
//! which an edit makes again with `Overwrite`), the delete of several (`DeleteParameters`), and what a parameter's own pages read
//! and write (its history, its labels). A page names its parameter in the input; each call is the service's own, made as the caller
//! `console`. Also how the list's filter tokens become `DescribeParameters` filters.

use serde_json::{Value, json};

use super::{Op, ops::free};

/// The parameters' pages.
pub(super) const OPS: &[Op] = &[
    free("PutParameter"),
    free("GetParameter"),
    free("DeleteParameter"),
    free("DeleteParameters"),
    free("GetParameterHistory"),
    free("LabelParameterVersion"),
    free("UnlabelParameterVersion"),
    free("AddTagsToResource"),
    free("RemoveTagsFromResource"),
    free("ListTagsForResource"),
];

/// How a name filter compares (`DescribeParameters` options a `Name` filter takes).
const OPTIONS: [&str; 3] = ["Equals", "BeginsWith", "Contains"];

/// The filter tokens of the list (`Name:BeginsWith:/larder/staging|Type:Equals:String`, joined by `|`; a name has no `|`) as the
/// `ParameterFilters` of `DescribeParameters`. A token the service has no filter for is left out.
#[must_use]
pub(super) fn filters(tokens: &str) -> Value {
    let list: Vec<Value> = tokens
        .split('|')
        .filter_map(|t| {
            let mut parts = t.splitn(3, ':');
            Some((parts.next()?, parts.next()?, parts.next()?))
        })
        .filter(|(key, option, value)| {
            !value.is_empty()
                && match *key {
                    "Name" => OPTIONS.contains(option),
                    "Type" => *option == "Equals",
                    _ => false,
                }
        })
        .map(|(key, option, value)| json!({"Key": key, "Option": option, "Values": [value]}))
        .collect();
    if list.is_empty() {
        json!({})
    } else {
        json!({"ParameterFilters": list})
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::filters;

    #[test]
    fn tokens_become_parameter_filters_and_what_the_service_does_not_take_is_dropped() {
        assert_eq!(
            filters(
                "Name:BeginsWith:/larder/staging|Type:Equals:String|Name:Regex:x|Tier:Equals:Advanced|Name:Equals:"
            ),
            json!({"ParameterFilters": [
                {"Key": "Name", "Option": "BeginsWith", "Values": ["/larder/staging"]},
                {"Key": "Type", "Option": "Equals", "Values": ["String"]},
            ]})
        );
        assert_eq!(filters(""), json!({}));
        assert_eq!(
            filters("Name:Contains:db"),
            json!({"ParameterFilters": [{"Key": "Name", "Option": "Contains", "Values": ["db"]}]})
        );
    }
}
