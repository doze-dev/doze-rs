//! Decoding a module's configuration from the core's `Value` tree, with a diagnostic at the exact path of anything
//! wrong. `#[derive(Spec)]` writes the `Spec` impls; the primitives here are what it calls.

use doze_module_proto::msg::error::Diagnostic;
use doze_module_proto::msg::hello::{BlockSchema, Schema, Type};
use doze_module_proto::msg::value::{Node, PathSeg, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A configuration block type: its schema and how to read it. Derive it.
pub trait Spec: Sized {
    /// The schema the core validates against and the registry documents.
    fn schema() -> Schema;
    /// The names of the block's labels (for a nested block type); empty for none.
    fn labels() -> Vec<String> {
        Vec::new()
    }
    /// Reads the block. Problems go to `cx`; `None` when the value cannot be built.
    fn decode(node: &Node, cx: &mut Decoding) -> Option<Self>;
}

/// A value an argument can hold.
pub trait Field: Sized {
    fn ty() -> Type;
    fn read(node: &Node, cx: &mut Decoding) -> Option<Self>;
    /// The value when the argument is absent and has no default: `None` means it is required.
    fn absent() -> Option<Self> {
        None
    }
}

/// The state of one decode: the declared engine version, where we are, what went wrong.
#[derive(Debug, Default)]
pub struct Decoding {
    /// The instance's declared engine version ("17"), for `since`/`until`.
    pub version: String,
    path: Vec<PathSeg>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Decoding {
    pub fn new(version: &str) -> Decoding {
        Decoding {
            version: version.to_string(),
            ..Decoding::default()
        }
    }

    /// Records a problem at the current path.
    pub fn error(&mut self, message: impl Into<String>, hint: Option<String>) {
        self.diagnostics.push(Diagnostic {
            path: self.path.clone(),
            message: message.into(),
            hint,
        });
    }

    pub(crate) fn path_push(&mut self, seg: PathSeg) {
        self.path.push(seg);
    }

    pub(crate) fn path_pop(&mut self) {
        self.path.pop();
    }

    fn within<T>(&mut self, seg: PathSeg, f: impl FnOnce(&mut Decoding) -> T) -> T {
        self.path.push(seg);
        let r = f(self);
        self.path.pop();
        r
    }

    /// The node as a map, or a diagnostic.
    pub fn map<'n>(&mut self, node: &'n Node, what: &str) -> Option<&'n BTreeMap<String, Node>> {
        match &node.value {
            Value::Map(m) => Some(m),
            _ => {
                self.error(format!("{what} must be a block or an object"), None);
                None
            }
        }
    }

    /// Flags keys the type does not declare (the core checks this too; this keeps a module honest on its own).
    pub fn only(&mut self, map: &BTreeMap<String, Node>, known: &[&str]) {
        for key in map.keys().filter(|k| !known.contains(&k.as_str())) {
            let hint = closest(key, known).map(|c| format!("did you mean `{c}`?"));
            self.within(PathSeg::Key(key.clone()), |cx| {
                cx.error(format!("unknown argument `{key}`"), hint)
            });
        }
    }

    fn major(&self) -> Option<u32> {
        self.version.split('.').next()?.parse().ok()
    }

    /// Reads argument `name`: the value if present (checked against `since`/`until`), else the default, else
    /// `T::absent()`, else a "required" diagnostic.
    pub fn field<T: Field>(
        &mut self,
        map: &BTreeMap<String, Node>,
        name: &str,
        default: Option<Value>,
        bounds: (Option<u32>, Option<u32>),
    ) -> Option<T> {
        let Some(node) = map.get(name) else {
            if let Some(d) = default {
                return T::read(&Node::bare(d), self);
            }
            let v = T::absent();
            if v.is_none() {
                self.error(format!("the argument `{name}` is required"), None);
            }
            return v;
        };
        self.within(PathSeg::Key(name.to_string()), |cx| {
            let (since, until) = bounds;
            if let (Some(since), Some(major)) = (since, cx.major())
                && major < since
            {
                cx.error(
                    format!(
                        "`{name}` needs engine version {since} or later (declared {})",
                        cx.version
                    ),
                    None,
                );
            }
            if let (Some(until), Some(major)) = (until, cx.major())
                && major >= until
            {
                cx.error(
                    format!(
                        "`{name}` was removed in engine version {until} (declared {})",
                        cx.version
                    ),
                    None,
                );
            }
            T::read(node, cx)
        })
    }

    /// Reads the nested blocks `name` (a list of maps; absent means none), checking labels are unique.
    pub fn blocks<T: Spec>(&mut self, map: &BTreeMap<String, Node>, name: &str) -> Option<Vec<T>> {
        let Some(node) = map.get(name) else {
            return Some(Vec::new());
        };
        self.within(PathSeg::Key(name.to_string()), |cx| {
            let Value::List(items) = &node.value else {
                cx.error(format!("`{name}` must be a list of blocks"), None);
                return None;
            };
            let labels = T::labels();
            let mut seen = BTreeSet::new();
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let idx = u32::try_from(i).unwrap_or(u32::MAX);
                let key = label_key(item, &labels);
                cx.within(PathSeg::Index(idx), |cx| {
                    if !labels.is_empty() && !seen.insert(key.clone()) {
                        cx.error(format!("{name} {key} is declared more than once"), None);
                    }
                    out.push(T::decode(item, cx));
                });
            }
            out.into_iter().collect()
        })
    }
}

fn label_key(item: &Node, labels: &[String]) -> String {
    labels
        .iter()
        .map(|l| item.get(l).map(text).unwrap_or_default())
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn text(n: &Node) -> String {
    match &n.value {
        Value::String(s) | Value::Number(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// The known name nearest to `key` (edit distance ≤ 2), for "did you mean".
pub fn closest<'a>(key: &str, known: &[&'a str]) -> Option<&'a str> {
    known
        .iter()
        .map(|k| (distance(key, k), *k))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, k)| k)
}

/// Edits between `a` and `b`, a swap of neighbouring letters counting as one (optimal string alignment): `lur` is one
/// edit from `lru`, the commonest typo, not two, so it is not tied with `lfu`.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let w = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * w];
    let get = |d: &[usize], i: usize, j: usize| d.get(i * w + j).copied().unwrap_or(usize::MAX / 2);
    for i in 0..=a.len() {
        if let Some(c) = d.get_mut(i * w) {
            *c = i;
        }
    }
    for j in 0..=b.len() {
        if let Some(c) = d.get_mut(j) {
            *c = j;
        }
    }
    for (i, ca) in a.iter().enumerate() {
        for (j, cb) in b.iter().enumerate() {
            let mut best = (get(&d, i, j + 1) + 1)
                .min(get(&d, i + 1, j) + 1)
                .min(get(&d, i, j) + usize::from(ca != cb));
            let swapped = i > 0 && j > 0 && a.get(i - 1) == Some(cb) && b.get(j - 1) == Some(ca);
            if swapped {
                best = best.min(get(&d, i - 1, j - 1) + 1);
            }
            if let Some(c) = d.get_mut((i + 1) * w + j + 1) {
                *c = best;
            }
        }
    }
    get(&d, a.len(), b.len())
}

/// The schema entry for nested block type `T` named `name`.
pub fn block_schema<T: Spec>(name: &str, doc: &str) -> BlockSchema {
    BlockSchema {
        name: name.to_string(),
        labels: T::labels(),
        repeated: true,
        doc: doc.to_string(),
        schema: T::schema(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_swap_of_neighbours_is_one_edit_and_the_nearest_wins() {
        assert_eq!(distance("lur", "lru"), 1);
        assert_eq!(distance("lur", "lfu"), 2);
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("maxmemory", "maxmemory"), 0);
        let policies = ["allkeys-lfu", "allkeys-lru", "noeviction"];
        assert_eq!(closest("allkeys-lur", &policies), Some("allkeys-lru"));
        assert_eq!(closest("something-else", &policies), None);
    }
}
