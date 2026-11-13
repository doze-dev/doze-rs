//! What the pages inside a stack show, computed from the hub (PLAN.md §4.1, Go `internal/desk/view`): an instance's
//! page, its logs (and every instance's, merged), a module's console tab. Plain data with no GPUI in it, each
//! `PartialEq`, so the app redraws only when a page changed. The Go app's view goldens hold these to Go's
//! (`tests/goldens`).

pub mod console;
pub mod filter;
pub mod instance;
pub mod logindex;
mod logread;
pub mod logs;
pub mod parts;
mod resolve;
pub mod runs;
pub mod side;
pub mod snippets;
pub mod time;

/// The colour class a piece of text is drawn in: the words' tones and the accent (doze's own: a woke event, the
/// bat's `zzz`, a match).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ink {
    Good,
    Warn,
    Cool,
    Bad,
    Dim,
    #[default]
    Text,
    Bright,
    Accent,
}

impl From<doze_display::Tone> for Ink {
    fn from(t: doze_display::Tone) -> Ink {
        use doze_display::Tone;
        match t {
            Tone::Good => Ink::Good,
            Tone::Warn => Ink::Warn,
            Tone::Cool => Ink::Cool,
            Tone::Bad => Ink::Bad,
            Tone::Dim => Ink::Dim,
            Tone::Text => Ink::Text,
            Tone::Bright => Ink::Bright,
        }
    }
}
