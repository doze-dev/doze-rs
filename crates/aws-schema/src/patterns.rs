//! Pattern constraints: compiled on first use, one slot per constraint, nothing at load.
//! A pattern `regex-lite` cannot compile needs a hand predicate; the registry below is where they live. A pattern with neither
//! is recorded as unenforced and never rejects (the same behaviour as the Go build, which lists them), and counted.

use crate::tables::Tables;
use regex_lite::Regex;
use std::sync::OnceLock;

enum Slot {
    Regex(Regex),
    Hand(Predicate),
    Unenforced,
}

/// One lazily filled slot per constraint.
type Slots = Vec<OnceLock<Slot>>;

#[derive(Default)]
pub struct Patterns {
    slots: Slots,
}

impl std::fmt::Debug for Patterns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Patterns")
            .field("slots", &self.slots.len())
            .finish()
    }
}

/// Hand predicates, keyed by the exact pattern text. Each is an overlay entry: it names the model pattern it replaces.
type Predicate = fn(&str) -> bool;
const HAND: &[(&str, Predicate)] = &[(r"^(?!\s*$).+$", not_blank)];

fn not_blank(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

impl Patterns {
    #[must_use]
    pub fn new(t: &Tables) -> Self {
        Self {
            slots: (0..t.n_cons()).map(|_| OnceLock::new()).collect(),
        }
    }

    /// Does `s` match the pattern of constraint `cons`? An unenforceable pattern accepts.
    #[must_use]
    pub fn matches(&self, cons: u32, pattern: &str, s: &str) -> bool {
        let Some(cell) = self.slots.get(cons as usize) else {
            return true;
        };
        let slot = cell.get_or_init(|| {
            if let Some((_, f)) = HAND.iter().find(|(p, _)| *p == pattern) {
                Slot::Hand(*f)
            } else {
                Regex::new(pattern).map_or(Slot::Unenforced, Slot::Regex)
            }
        });
        match slot {
            Slot::Regex(r) => r.is_match(s),
            Slot::Hand(f) => f(s),
            Slot::Unenforced => true,
        }
    }
}
