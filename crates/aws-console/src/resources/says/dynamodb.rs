//! DynamoDB: how a table's page speaks.

use serde_json::Value;

use super::super::page::list;
use super::super::path;
use super::super::{Page, Read, Verb};
use super::status;
use crate::model::Badge;

/// `1204` as `1,204`.
#[must_use]
pub fn grouped(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// The indexes a table has at `at` of DescribeTable: each one's name and status.
fn indexes(r: &Read<'_>, at: &str) -> Vec<(String, String)> {
    path::get(r.out, at)
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|i| {
            let text = |m: &str| i.get(m).and_then(Value::as_str).unwrap_or_default();
            // A local index has no status of its own: it is as active as its table.
            let status = Some(text("IndexStatus"))
                .filter(|s| !s.is_empty())
                .unwrap_or("ACTIVE");
            (text("IndexName").to_owned(), title(status))
        })
        .collect()
}

/// `ACTIVE` as `Active`.
fn title(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let mut c = lower.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// `One global secondary index, by-customer, is Active.`
fn about_indexes(found: &[(String, String)], kind: &str) -> Option<String> {
    let (first, rest) = found.split_first()?;
    let names: Vec<String> = found.iter().map(|(n, _)| n.clone()).collect();
    if rest.iter().all(|(_, s)| *s == first.1) {
        let count = match found.len() {
            1 => format!("One {kind} index"),
            n => format!("{n} {kind} indexes"),
        };
        let verb = if found.len() == 1 { "is" } else { "are" };
        Some(format!("{count}, {}, {verb} {}.", list(&names), first.1))
    } else {
        let each: Vec<String> = found.iter().map(|(n, s)| format!("{n} is {s}")).collect();
        Some(format!("{}.", list(&each)))
    }
}

/// `orders-table holds 1,204 items. One global secondary index, by-customer, is Active. No DynamoDB stream is on.`
fn sentence(r: &Read<'_>) -> Option<String> {
    let items = r.number("Table.ItemCount")?;
    let name = r.name;
    let mut out = format!(
        "{name} holds {} {}.",
        grouped(items),
        if items == 1 { "item" } else { "items" }
    );
    let global = indexes(r, "Table.GlobalSecondaryIndexes");
    let local = indexes(r, "Table.LocalSecondaryIndexes");
    if let Some(s) = about_indexes(&global, "global secondary") {
        out.push(' ');
        out.push_str(&s);
    }
    if let Some(s) = about_indexes(&local, "local secondary") {
        out.push(' ');
        out.push_str(&s);
    }
    if global.is_empty() && local.is_empty() {
        out.push_str(" It has no secondary index.");
    }
    if r.flag("Table.StreamSpecification.StreamEnabled") {
        let view = r
            .text("Table.StreamSpecification.StreamViewType")
            .unwrap_or_default();
        out.push_str(&format!(" A DynamoDB stream is on, with view type {view}."));
    } else {
        out.push_str(" No DynamoDB stream is on.");
    }
    Some(out)
}

fn badges(r: &Read<'_>) -> Vec<Badge> {
    status(r.text("Table.TableStatus"))
}

/// A table's header: what it is doing, and the way into its items.
pub const TABLE: Page = Page {
    sentence: Some(sentence),
    badges: Some(badges),
    verbs: &[Verb::ui("explore", "Explore table items")],
    summary: None,
};

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(318), "318");
        assert_eq!(grouped(1204), "1,204");
        assert_eq!(grouped(4_462_000), "4,462,000");
    }
}
