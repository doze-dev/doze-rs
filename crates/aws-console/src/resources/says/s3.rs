//! S3: how a bucket's page speaks, and the line under the list of buckets.

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::page::{list, plural};
use super::super::path;
use super::super::{Page, Read};
use crate::model::ResourceRow;

/// The three kinds of destination a bucket's notification configuration names, with the member that holds each one's ARN.
const DESTINATIONS: [(&str, &str); 3] = [
    ("LambdaFunctionConfigurations", "LambdaFunctionArn"),
    ("QueueConfigurations", "QueueArn"),
    ("TopicConfigurations", "TopicArn"),
];

/// How many entries the list at `at` of the answers has.
fn count(r: &Read<'_>, at: &str) -> usize {
    path::get(r.out, at)
        .and_then(|v| v.as_array().map(Vec::len))
        .unwrap_or(0)
}

/// The names of the destinations of the notification rules that fire on `event` (`ObjectCreated`), each once, in the order the
/// configuration names them.
fn notified(r: &Read<'_>, event: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (rules, arn) in DESTINATIONS {
        let Some(Value::Array(rules)) = path::get(r.out, rules) else {
            continue;
        };
        for rule in &rules {
            let fires = rule
                .get("Events")
                .and_then(Value::as_array)
                .is_some_and(|e| {
                    e.iter()
                        .filter_map(Value::as_str)
                        .any(|e| e.starts_with(&format!("s3:{event}:")))
                });
            let to = rule.get(arn).and_then(Value::as_str).map(path::tail);
            if let (true, Some(to)) = (fires, to)
                && !out.contains(&to)
            {
                out.push(to);
            }
        }
    }
    out
}

/// `product-images holds 4 objects and 2 folders at the top level, with versioning on. New objects notify resize-product-image.`
fn sentence(r: &Read<'_>) -> Option<String> {
    // A bucket whose listing was refused says nothing of what it holds (the page's own tab says why).
    path::get(r.out, "KeyCount")?;
    let name = r.name;
    let objects = i64::try_from(count(r, "Contents")).unwrap_or(i64::MAX);
    let folders = i64::try_from(count(r, "CommonPrefixes")).unwrap_or(i64::MAX);
    let more = r.flag("IsTruncated");
    let at_least = if more { "at least " } else { "" };
    let mut out = if objects + folders == 0 {
        format!("{name} is empty")
    } else {
        let mut what = vec![format!(
            "{at_least}{}",
            plural(objects, "object", "objects")
        )];
        if folders > 0 {
            what.push(plural(folders, "folder", "folders"));
        }
        format!("{name} holds {} at the top level", list(&what))
    };
    match r.text("Status").as_deref() {
        Some("Enabled") => out.push_str(", with versioning on"),
        Some("Suspended") => out.push_str(", with versioning suspended"),
        _ => {}
    }
    out.push('.');
    for (event, said) in [
        ("ObjectCreated", "New objects notify"),
        ("ObjectRemoved", "Removed objects notify"),
    ] {
        let to = notified(r, event);
        if !to.is_empty() {
            out.push_str(&format!(" {said} {}.", list(&to)));
        }
    }
    Some(out)
}

/// What happens, in words, when an event like `s3:ObjectCreated:Put` fires.
fn when(event: &str) -> String {
    let kind = event.split(':').nth(1).unwrap_or(event);
    match kind {
        "ObjectCreated" => "an object is created".to_owned(),
        "ObjectRemoved" => "an object is removed".to_owned(),
        "" => "an event happens".to_owned(),
        other => format!("a {other} event happens"),
    }
}

/// What one listed bucket is worth saying: that it keeps versions, and whom it notifies and when. `None` for a bucket with neither.
fn clause(row: &ResourceRow) -> Option<String> {
    let cell = |i: usize| row.cells.get(i).cloned().flatten();
    let mut bits = Vec::new();
    if cell(2).as_deref() == Some("Enabled") {
        bits.push("versioning on".to_owned());
    }
    let to = [(3, "Lambda function"), (4, "SQS queue"), (5, "SNS topic")]
        .into_iter()
        .find_map(|(i, kind)| cell(i).map(|name| format!("{kind} {name}")));
    if let Some(to) = to {
        let on = cell(6).map(|e| when(&e)).unwrap_or_default();
        bits.push(if on.is_empty() {
            format!("notifies {to}")
        } else {
            format!("notifies {to} when {on}")
        });
    }
    (!bits.is_empty()).then(|| format!("{} has {}", row.name, bits.join(" and ")))
}

/// `4 buckets in this account, all in us-east-1. product-images has versioning on and notifies Lambda function
/// resize-product-image when an object is created.`
fn summary(rows: &[ResourceRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let regions: BTreeSet<String> = rows
        .iter()
        .filter_map(|r| r.cells.first().cloned().flatten())
        .collect();
    let n = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let mut out = format!("{} in this account", plural(n, "bucket", "buckets"));
    match regions.len() {
        0 => {}
        1 => {
            let one = regions.into_iter().next().unwrap_or_default();
            out.push_str(&format!(", {}in {one}", if n == 1 { "" } else { "all " }));
        }
        k => out.push_str(&format!(", in {k} Regions")),
    }
    out.push('.');
    // The first two buckets with something to say; how many more there are.
    let said: Vec<String> = rows.iter().filter_map(clause).collect();
    for c in said.iter().take(2) {
        out.push_str(&format!(" {c}."));
    }
    if said.len() > 2 {
        let more = i64::try_from(said.len() - 2).unwrap_or(i64::MAX);
        let (what, verb) = if more == 1 {
            ("bucket", "keeps")
        } else {
            ("buckets", "keep")
        };
        out.push_str(&format!(" {more} more {what} {verb} versions or notify."));
    }
    Some(out)
}

/// A bucket's header: what it holds at the top level, what it notifies. (Its verbs are the page's own: Empty and Delete are
/// actions of the list and of the Actions menu.)
pub const BUCKET: Page = Page {
    sentence: Some(sentence),
    badges: None,
    verbs: &[],
    summary: Some(summary),
};

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::ResourceRow;

    fn say(out: &Value) -> Option<String> {
        sentence(&Read {
            out,
            name: "product-images",
            subs: &[],
        })
    }

    #[test]
    fn a_bucket_says_what_it_holds_and_whom_it_tells() {
        let out = json!({
            "KeyCount": 6,
            "Contents": [{"Key": "a.jpg"}, {"Key": "b.jpg"}, {"Key": "c.jpg"}, {"Key": "d.jpg"}],
            "CommonPrefixes": [{"Prefix": "banners/"}, {"Prefix": "catalog/"}],
            "Status": "Enabled",
            "LambdaFunctionConfigurations": [{"Id": "x", "LambdaFunctionArn": "arn:aws:lambda:us-east-1:1:function:resize-product-image", "Events": ["s3:ObjectCreated:*"]}],
            "QueueConfigurations": [{"Id": "y", "QueueArn": "arn:aws:sqs:us-east-1:1:gone", "Events": ["s3:ObjectRemoved:Delete"]}],
        });
        assert_eq!(
            say(&out).unwrap(),
            "product-images holds 4 objects and 2 folders at the top level, with versioning on. \
             New objects notify resize-product-image. Removed objects notify gone."
        );
    }

    #[test]
    fn an_empty_or_long_or_unversioned_bucket_is_said_as_it_is() {
        assert_eq!(
            say(&json!({"KeyCount": 0})).unwrap(),
            "product-images is empty."
        );
        assert_eq!(
            say(&json!({"KeyCount": 1000, "IsTruncated": true, "Contents": [{"Key": "a"}], "Status": "Suspended"})).unwrap(),
            "product-images holds at least 1 object at the top level, with versioning suspended."
        );
        // A listing that was refused leaves the line out.
        assert_eq!(say(&json!({"Status": "Enabled"})), None);
    }

    #[test]
    fn the_list_says_how_many_buckets_and_where() {
        let row = |region: &str| ResourceRow {
            name: String::new(),
            label: String::new(),
            id: String::new(),
            cells: vec![Some(region.to_owned())],
            link: None,
        };
        assert_eq!(summary(&[]), None);
        assert_eq!(
            summary(&[row("us-east-1"), row("us-east-1")]).unwrap(),
            "2 buckets in this account, all in us-east-1."
        );
        assert_eq!(
            summary(&[row("us-east-1")]).unwrap(),
            "1 bucket in this account, in us-east-1."
        );
        assert_eq!(
            summary(&[row("us-east-1"), row("eu-west-1")]).unwrap(),
            "2 buckets in this account, in 2 Regions."
        );
    }

    #[test]
    fn the_list_names_the_buckets_that_keep_versions_or_notify() {
        let row = |name: &str, versioning: &str, lambda: Option<&str>| ResourceRow {
            name: name.to_owned(),
            label: String::new(),
            id: String::new(),
            cells: vec![
                Some("us-east-1".to_owned()),
                None,
                Some(versioning.to_owned()),
                lambda.map(str::to_owned),
                None,
                None,
                lambda.map(|_| "s3:ObjectCreated:*".to_owned()),
            ],
            link: None,
        };
        assert_eq!(
            summary(&[
                row("product-images", "Enabled", Some("resize-product-image")),
                row("receipts", "Disabled", None),
            ])
            .unwrap(),
            "2 buckets in this account, all in us-east-1. product-images has versioning on and notifies Lambda function \
             resize-product-image when an object is created."
        );
        // Past two, the rest are counted.
        let many: Vec<ResourceRow> = ["a", "b", "c", "d"]
            .iter()
            .map(|n| row(n, "Enabled", None))
            .collect();
        assert!(
            summary(&many)
                .unwrap()
                .ends_with(" 2 more buckets keep versions or notify.")
        );
        // A bucket whose settings could not be read says nothing of them.
        let bare = ResourceRow {
            cells: vec![Some("us-east-1".to_owned())],
            ..row("x", "", None)
        };
        assert_eq!(
            summary(&[bare]).unwrap(),
            "1 bucket in this account, in us-east-1."
        );
    }
}
