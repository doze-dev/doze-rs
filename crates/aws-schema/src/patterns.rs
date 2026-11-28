//! Pattern constraints: compiled on first use, one slot per constraint, nothing at load.
//! A pattern `regex-lite` cannot compile needs a hand predicate; the registry below is where they live. A pattern with neither
//! is recorded as unenforced and never rejects (the same behaviour as the Go build, which lists them), and counted.
//!
//! There is one set of slots per model, however many hold a [`Patterns`] for it (the gateway, the peer bus and the service each
//! do): a pattern is compiled once. The slots themselves are allocated when the model's first pattern is matched, so a model no
//! request has used costs a few bytes (all models' slots were about 118 KB, and the gateway and the bus held a copy each at idle).

use crate::tables::Tables;
use regex_lite::Regex;
use std::sync::{Mutex, OnceLock};

enum Slot {
    Regex(Regex),
    Hand(Predicate),
    Unenforced,
}

/// One lazily filled slot per constraint.
type Slots = Box<[OnceLock<Slot>]>;

/// A model's slots: how many, and the slots once a pattern of the model has been matched.
struct Model {
    n: u32,
    slots: OnceLock<Slots>,
}

/// Every model's slots, by the address of its tables (a model's tables are a `static`).
static MODELS: Mutex<Vec<(usize, &'static Model)>> = Mutex::new(Vec::new());

/// The patterns of one model: a handle on its one set of slots.
#[derive(Clone, Copy)]
pub struct Patterns {
    model: &'static Model,
}

impl std::fmt::Debug for Patterns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Patterns")
            .field("slots", &self.model.n)
            .field("allocated", &self.model.slots.get().is_some())
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
    /// The patterns of the model whose tables are `t`: the same slots every time it is asked for.
    #[must_use]
    pub fn new(t: &'static Tables) -> Self {
        let key = std::ptr::from_ref(t) as usize;
        let mut models = MODELS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, model)) = models.iter().find(|(k, _)| *k == key) {
            return Self { model };
        }
        // WHY: a model's slots live as long as the process, as its tables do; one small leak per model, once.
        let model: &'static Model = Box::leak(Box::new(Model {
            n: t.n_cons(),
            slots: OnceLock::new(),
        }));
        models.push((key, model));
        Self { model }
    }

    /// Whether the model's slots have been allocated (a pattern of it was matched).
    #[must_use]
    pub fn allocated(&self) -> bool {
        self.model.slots.get().is_some()
    }

    /// Does `s` match the pattern of constraint `cons`? An unenforceable pattern accepts.
    #[must_use]
    pub fn matches(&self, cons: u32, pattern: &str, s: &str) -> bool {
        let slots = self
            .model
            .slots
            .get_or_init(|| (0..self.model.n).map(|_| OnceLock::new()).collect());
        let Some(cell) = slots.get(cons as usize) else {
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
