//! A value viewer's asks, decided here so the rules are tested where they live: what it shows follows the page (a new
//! pick or leaf is asked for), an ask never wakes an instance unless the person pressed to open the value, a refusal
//! for sleeping or starting is asked again as the instance wakes, and an answer to an ask no longer wanted is dropped.
//! The app runs each [`Ask`] on the hub off the UI thread and hands the answer back ([`Viewer::answered`]).

use super::value::{Refusal, ValueDoc};
use doze_control::ValueView;
use std::collections::BTreeMap;

/// What a value viewer is to show: the panel asked, the row (a table's cells or `{"id": leaf}`), its identity and its
/// name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Want {
    pub panel: String,
    pub row: BTreeMap<String, String>,
    pub ident: String,
    pub label: String,
}

/// One ask of the daemon's `value`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub panel: String,
    pub row: BTreeMap<String, String>,
    pub offset: u64,
    /// Only a person's press to open the value sets it.
    pub wake: bool,
    /// Which ask it is: an answer to an older one is dropped.
    pub seq: u64,
}

/// One value viewer's state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Viewer {
    pub want: Option<Want>,
    pub doc: Option<ValueDoc>,
    pub refused: Option<Refusal>,
    pub loading: bool,
    asked: u64,
}

impl Viewer {
    /// The page was resolved and this is what the viewer is to show now. Asks (never waking) when it changed, or when
    /// the instance `woke` and the last ask was refused for sleeping or starting. The same value with a better name
    /// (its folder was read since) only takes the name.
    pub fn follow(&mut self, want: Option<Want>, woke: bool) -> Option<Ask> {
        let same = self.want.as_ref().map(|w| &w.ident) == want.as_ref().map(|w| &w.ident);
        if same {
            if let (Some(w), Some(new)) = (self.want.as_mut(), want.as_ref()) {
                w.label.clone_from(&new.label);
            }
            let retry = woke
                && self
                    .refused
                    .as_ref()
                    .is_some_and(|r| r.asleep() || r.starting());
            return retry.then(|| self.ask(0, false)).flatten();
        }
        *self = Viewer {
            want,
            asked: self.asked,
            ..Viewer::default()
        };
        self.ask(0, false)
    }

    /// The person pressed "Open and wake it": the one ask that wakes a sleeping instance.
    pub fn open_waking(&mut self) -> Option<Ask> {
        self.ask(0, true)
    }

    /// "Try again" (an instance starting, a module that failed to answer): asked again, not woken.
    pub fn retry(&mut self) -> Option<Ask> {
        self.ask(0, false)
    }

    /// "Load the rest": the next part, from where the body ends.
    pub fn rest(&mut self) -> Option<Ask> {
        let from = self.doc.as_ref().filter(|d| d.more)?.next_offset();
        self.ask(from, false)
    }

    fn ask(&mut self, offset: u64, wake: bool) -> Option<Ask> {
        let w = self.want.as_ref()?;
        self.asked += 1;
        self.loading = true;
        if offset == 0 {
            self.refused = None;
        }
        Some(Ask {
            panel: w.panel.clone(),
            row: w.row.clone(),
            offset,
            wake,
            seq: self.asked,
        })
    }

    /// The daemon answered `ask`; `true` when it was taken (a first part is a new value: its folds start over).
    pub fn answered(&mut self, ask: &Ask, got: Result<ValueView, Refusal>) -> bool {
        if ask.seq != self.asked {
            return false;
        }
        self.loading = false;
        match got {
            Ok(part) if ask.offset > 0 => {
                if let Some(d) = &mut self.doc {
                    d.add(&part);
                }
                false
            }
            Ok(part) => {
                self.doc = Some(ValueDoc::first(&part));
                true
            }
            Err(r) => {
                self.refused = Some(r);
                false
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn want(ident: &str) -> Option<Want> {
        Some(Want {
            panel: "keys".into(),
            row: BTreeMap::from([("id".into(), ident.into())]),
            ident: ident.into(),
            label: ident.into(),
        })
    }

    fn asleep() -> Refusal {
        Refusal {
            message: "db is asleep. Opening a value wakes it.".into(),
            code: doze_control::fault::ASLEEP.into(),
        }
    }

    #[test]
    fn only_the_press_wakes_and_a_wake_asks_again() {
        let mut v = Viewer::default();
        let a = v.follow(want("a"), false).unwrap();
        assert!(!a.wake, "following a pick never wakes");
        assert_eq!(
            v.follow(want("a"), false),
            None,
            "the same value is not asked again"
        );
        v.answered(&a, Err(asleep()));
        assert_eq!(
            v.follow(want("a"), false),
            None,
            "asleep: nothing until a press or a wake"
        );
        let woke = v.follow(want("a"), true).unwrap();
        assert!(!woke.wake, "asked again as it wakes, not woken by it");
        let pressed = v.open_waking().unwrap();
        assert!(pressed.wake, "the press is the one ask that wakes");
        assert!(
            !v.answered(&woke, Err(asleep())),
            "an older ask's answer is dropped"
        );
        assert!(v.refused.is_none());
    }

    #[test]
    fn parts_add_up_and_a_new_pick_starts_over() {
        let mut v = Viewer::default();
        let a = v.follow(want("a"), false).unwrap();
        let mut part = ValueView {
            media: "text".into(),
            size: 6,
            more: true,
            ..ValueView::default()
        };
        part.set_body(b"abc");
        assert!(v.answered(&a, Ok(part.clone())));
        let rest = v.rest().unwrap();
        assert_eq!((rest.offset, rest.wake), (3, false));
        part.offset = 3;
        part.more = false;
        part.set_body(b"def");
        v.answered(&rest, Ok(part));
        assert_eq!(v.doc.as_ref().unwrap().text(), "abcdef");
        assert_eq!(v.rest(), None, "nothing more to load");
        v.follow(want("b"), false).unwrap();
        assert!(v.doc.is_none(), "a new pick starts over");
    }
}
