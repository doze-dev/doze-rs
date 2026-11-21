//! How a detail page speaks of its resource: a sentence built from its real fields, badges, the verbs of its header, and what it
//! is wired to. A kind says them with small functions over [`Read`] (what its detail's calls answered and its lists), so a
//! sentence can count, pick a word, and leave a clause out; nothing here guesses a value the service did not give.

use serde_json::Value;

use super::{Kind, path};
use crate::model::{Badge, FieldValue, Needs, ResourceRow, SubList, VerbSpec, Wired};

/// What a page reads a resource from.
#[derive(Debug, Clone, Copy)]
pub struct Read<'a> {
    /// What the detail's calls answered, merged ([`super::Kind::detail`]); `$id` and `$name` are in it.
    pub out: &'a Value,
    /// Its name in the address.
    pub name: &'a str,
    /// Its lists, read ([`super::Kind::subs`]).
    pub subs: &'a [SubList],
}

impl Read<'_> {
    /// The text at `path` of the answers.
    #[must_use]
    pub fn text(&self, at: &str) -> Option<String> {
        path::text(self.out, at).filter(|t| !t.is_empty())
    }

    /// The whole number at `path` (an SQS count arrives as text).
    #[must_use]
    pub fn number(&self, at: &str) -> Option<i64> {
        self.text(at)?.parse().ok()
    }

    /// Whether the value at `path` is `true`.
    #[must_use]
    pub fn flag(&self, at: &str) -> bool {
        self.text(at)
            .is_some_and(|t| t.eq_ignore_ascii_case("true"))
    }

    /// The list titled `title`, when it was read without a refusal.
    #[must_use]
    pub fn sub(&self, title: &str) -> Option<&SubList> {
        self.subs
            .iter()
            .find(|s| s.title == title && s.error.is_none())
    }

    /// How many rows the list titled `title` has (`None` when it could not be read).
    #[must_use]
    pub fn rows(&self, title: &str) -> Option<usize> {
        self.sub(title).map(|s| s.rows.len())
    }
}

/// `1 message`, `2 messages`.
#[must_use]
pub fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `a`, `a and b`, `a, b and c`.
#[must_use]
pub fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// What a list says of the rows of its page.
pub type Summary = fn(&[ResourceRow]) -> Option<String>;

/// A verb of a detail's header.
#[derive(Debug, Clone, Copy)]
pub struct Verb {
    pub id: &'static str,
    pub label: &'static str,
    pub danger: bool,
    /// The operation of the kind's pages it makes ([`super::Deep`]); empty for a verb the page handles itself.
    pub op: &'static str,
    pub needs: Option<Needs>,
    /// Shown only when this holds of the resource; always when absent.
    pub when: Option<fn(&Read<'_>) -> bool>,
}

impl Verb {
    /// A verb the page handles itself (it opens a form or a tab).
    #[must_use]
    pub const fn ui(id: &'static str, label: &'static str) -> Self {
        Self {
            id,
            label,
            danger: false,
            op: "",
            needs: None,
            when: None,
        }
    }

    /// A verb that makes `op` on the resource after the page's `needs`.
    #[must_use]
    pub const fn call(
        id: &'static str,
        label: &'static str,
        op: &'static str,
        needs: Needs,
    ) -> Self {
        Self {
            op,
            needs: Some(needs),
            ..Self::ui(id, label)
        }
    }

    /// The same, drawn as one that destroys.
    #[must_use]
    pub const fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// The same, shown only when `when` holds.
    #[must_use]
    pub const fn when(mut self, when: fn(&Read<'_>) -> bool) -> Self {
        self.when = Some(when);
        self
    }
}

/// How a kind's detail speaks of a resource.
#[derive(Debug, Clone, Copy)]
pub struct Page {
    /// The one line under the title; `None` from it leaves the line out.
    pub sentence: Option<fn(&Read<'_>) -> Option<String>>,
    pub badges: Option<fn(&Read<'_>) -> Vec<Badge>>,
    pub verbs: &'static [Verb],
    /// The one line a list says of the rows of its page.
    pub summary: Option<Summary>,
}

impl Page {
    /// Says nothing: the kinds that have not been given a sentence.
    pub const NONE: Self = Self {
        sentence: None,
        badges: None,
        verbs: &[],
        summary: None,
    };
}

/// The sentence, badges and verbs of `kind`'s resource.
pub(super) fn speak(kind: &Kind, read: &Read<'_>) -> (Option<String>, Vec<Badge>, Vec<VerbSpec>) {
    let page = &kind.page;
    let sentence = page.sentence.and_then(|f| f(read));
    let badges = page.badges.map(|f| f(read)).unwrap_or_default();
    let verbs = page
        .verbs
        .iter()
        .filter(|v| v.when.is_none_or(|w| w(read)))
        .map(|v| VerbSpec {
            id: v.id.to_owned(),
            label: v.label.to_owned(),
            danger: v.danger,
            op: (!v.op.is_empty()).then(|| v.op.to_owned()),
            needs: v.needs,
        })
        .collect();
    (sentence, badges, verbs)
}

/// The most rows of one list the strip names.
const MOST_WIRED: usize = 8;

/// What the resource is wired to: each field that links to another resource, and the rows of each list whose rows link.
#[must_use]
pub fn wired(fields: &[FieldValue], subs: &[SubList]) -> Vec<Wired> {
    let mut out: Vec<Wired> = fields
        .iter()
        .filter_map(|f| {
            Some(Wired {
                label: f.label.clone(),
                to: f.link.clone()?,
                note: None,
            })
        })
        .collect();
    for sub in subs {
        out.extend(
            sub.rows
                .iter()
                .filter_map(|r| r.link.clone())
                .take(MOST_WIRED)
                .map(|to| Wired {
                    label: sub.title.clone(),
                    to,
                    note: None,
                }),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{list, plural};

    #[test]
    fn words_agree_with_their_counts() {
        assert_eq!(plural(1, "message", "messages"), "1 message");
        assert_eq!(plural(0, "message", "messages"), "0 messages");
        assert_eq!(list(&[]), "");
        assert_eq!(list(&["a".into()]), "a");
        assert_eq!(list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(list(&["a".into(), "b".into(), "c".into()]), "a, b and c");
    }
}
