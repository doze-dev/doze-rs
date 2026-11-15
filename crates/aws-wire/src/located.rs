//! Where in a request a failed model check sits, named as AWS's Coral front end names it.
//!
//! The decoder reports the member that broke its constraint, not the way down to it. AWS names the whole way: a bad filter key
//! is `filters.1.member.key` (Secrets Manager and SSM, recorded 2026-11-15), and a bad value in a list of strings is the list,
//! printed as Java prints one: `Value '[Nonsense]' at 'operations' failed to satisfy constraint: Member must satisfy constraint:
//! [Member must satisfy enum value set: [...]]` (KMS, the same day). Finding the way is done again, only on the refusal path, by
//! reading the body leniently and walking it to the member that holds the bad value; the decoder stays as fast as it was.

use aws_schema::arena::{Arena, NIL, Val};
use aws_schema::json::{Violation, decode_lenient};
use aws_schema::patterns::Patterns;
use aws_schema::tables::{K_LIST, K_STRUCT, K_UNION, Tables};

use crate::ApiError;

/// The way to a failed member.
#[derive(Debug, PartialEq, Eq)]
struct Located {
    /// `filters.1.member.key`.
    path: String,
    /// For a member of a list of scalars: every element of that list, as text.
    list: Option<Vec<String>>,
}

/// `error` with the failed member named by its whole path, when the error is worded the Coral way (`Value '...' at 'name'
/// failed to satisfy constraint: Member must ...`) and the member is not at the top of the request. Any other error is returned
/// as it is.
#[must_use]
pub fn place(
    error: ApiError,
    tables: &Tables,
    patterns: &Patterns,
    shape: u32,
    body: &str,
    violation: &Violation,
) -> ApiError {
    let (Violation::Length { member, value }
    | Violation::Range { member, value }
    | Violation::Pattern { member, value }
    | Violation::Enum { member, value }) = violation
    else {
        return error;
    };
    let Some(at) = locate(tables, patterns, shape, body, *member, value.as_deref()) else {
        return error;
    };
    let simple = tables
        .member(*member)
        .map_or_else(String::new, |m| lower_first(m.name));
    let shown = value.as_deref().unwrap_or("null");
    let old_head = format!("Value '{shown}' at '{simple}' failed to satisfy constraint: ");
    let Some((before, rest)) = error.message.split_once(&old_head) else {
        return error;
    };
    let message = match &at.list {
        Some(items) => format!(
            "{before}Value '[{}]' at '{}' failed to satisfy constraint: Member must satisfy constraint: [{rest}]",
            items.join(", "),
            at.path
        ),
        None if at.path == simple => return error,
        None => format!(
            "{before}Value '{shown}' at '{}' failed to satisfy constraint: {rest}",
            at.path
        ),
    };
    ApiError { message, ..error }
}

/// `error` with its enum value set listed in `order`: the order AWS printed for one enum, where the reverse of the model's that
/// [`default_invalid`](crate::service::default_invalid) uses is not it (KMS's grant operations, Secrets Manager's filter keys).
#[must_use]
pub fn enum_order(error: ApiError, order: &[&str]) -> ApiError {
    const HEAD: &str = "enum value set: [";
    let Some(start) = error.message.find(HEAD).map(|i| i + HEAD.len()) else {
        return error;
    };
    let Some(len) = error.message.get(start..).and_then(|r| r.find(']')) else {
        return error;
    };
    let mut message = error.message.clone();
    message.replace_range(start..start + len, &order.join(", "));
    ApiError { message, ..error }
}

fn lower_first(name: &str) -> String {
    let mut out = name.to_owned();
    if let Some(first) = out.get_mut(..1) {
        first.make_ascii_lowercase();
    }
    out
}

fn locate(
    tables: &Tables,
    patterns: &Patterns,
    shape: u32,
    body: &str,
    member: u32,
    value: Option<&str>,
) -> Option<Located> {
    let mut arena = Arena::new();
    let root = decode_lenient(tables, patterns, shape, body, &mut arena).ok()?;
    let Val::Obj(first) = arena.node(root)?.val else {
        return None;
    };
    let walk = Walk {
        tables,
        arena: &arena,
        body,
        member,
        value,
    };
    walk.object(first, "")
}

struct Walk<'a> {
    tables: &'a Tables,
    arena: &'a Arena,
    body: &'a str,
    member: u32,
    value: Option<&'a str>,
}

impl Walk<'_> {
    fn text(&self, v: Val) -> Option<String> {
        match v {
            Val::Str(..) | Val::Esc(..) => self.arena.str_of(self.body, v).map(str::to_owned),
            Val::Int(n) => Some(n.to_string()),
            Val::Float(f) => Some(f.to_string()),
            Val::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    fn holds(&self, v: Val) -> bool {
        self.value
            .is_none_or(|want| self.text(v).as_deref() == Some(want))
    }

    /// The structure whose first member node is `first`, at `prefix` (empty, or ending in a dot).
    fn object(&self, first: u32, prefix: &str) -> Option<Located> {
        let mut at = first;
        while at != NIL {
            let node = self.arena.node(at)?;
            at = node.next;
            let Some(m) = self.tables.member(node.member) else {
                continue;
            };
            let path = format!("{prefix}{}", lower_first(m.name));
            if node.member == self.member && self.holds(node.val) {
                return Some(Located { path, list: None });
            }
            let found = match (m.kind, node.val) {
                (K_STRUCT | K_UNION, Val::Obj(inner)) => self.object(inner, &format!("{path}.")),
                (K_LIST, Val::Seq(inner)) => self.list(inner, m.target, &path),
                _ => None,
            };
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// A list whose element member is `elem` and whose first element node is `first`.
    fn list(&self, first: u32, elem: u32, path: &str) -> Option<Located> {
        let mut items = Vec::new();
        let (mut at, mut i, mut hit) = (first, 0usize, false);
        while at != NIL {
            let node = self.arena.node(at)?;
            at = node.next;
            i += 1;
            if let Val::Obj(inner) = node.val {
                let found = self.object(inner, &format!("{path}.{i}.member."));
                if found.is_some() {
                    return found;
                }
                continue;
            }
            hit |= elem == self.member && self.holds(node.val);
            items.push(self.text(node.val).unwrap_or_default());
        }
        hit.then(|| Located {
            path: path.to_owned(),
            list: Some(items),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_model_lambda::tables::TABLES;
    use aws_schema::json::decode;

    use super::*;
    use crate::service::default_invalid;

    fn refusal(body: &str) -> String {
        let shape = TABLES
            .op(TABLES.find_op("CreateFunction").unwrap())
            .unwrap()
            .input
            .unwrap();
        let pats = Patterns::new(&TABLES);
        let v = decode(&TABLES, &pats, shape, body, &mut Arena::new()).unwrap_err();
        place(
            default_invalid(&TABLES, &v),
            &TABLES,
            &pats,
            shape,
            body,
            &v,
        )
        .message
    }

    #[test]
    fn an_enum_set_is_listed_in_the_order_given() {
        let e = ApiError::new(
            400,
            "ValidationException",
            "Member must satisfy enum value set: [b, a]]",
        );
        assert_eq!(
            enum_order(e, &["a", "b"]).message,
            "Member must satisfy enum value set: [a, b]]"
        );
    }

    #[test]
    fn a_member_at_the_top_keeps_its_name() {
        let m = refusal(r#"{"Runtime":"nope"}"#);
        assert!(m.contains("Value 'nope' at 'runtime' failed"), "{m}");
    }

    #[test]
    fn a_member_of_a_structure_is_named_by_its_way() {
        let m = refusal(r#"{"TracingConfig":{"Mode":"nope"}}"#);
        assert!(
            m.contains("Value 'nope' at 'tracingConfig.mode' failed"),
            "{m}"
        );
    }

    #[test]
    fn a_member_inside_a_list_of_structures_is_numbered_from_one() {
        let m = refusal(r#"{"FileSystemConfigs":[{"Arn":"bad","LocalMountPath":"/mnt/x"}]}"#);
        assert!(
            m.contains("at 'fileSystemConfigs.1.member.arn' failed"),
            "{m}"
        );
    }

    #[test]
    fn a_bad_element_of_a_list_of_strings_is_shown_as_the_list() {
        let m = refusal(r#"{"Architectures":["nope"]}"#);
        assert!(
            m.starts_with("1 validation error detected: Value '[nope]' at 'architectures' failed to satisfy constraint: Member must satisfy constraint: [Member must satisfy enum value set: ["),
            "{m}"
        );
        assert!(m.ends_with("]]"), "{m}");
    }
}
