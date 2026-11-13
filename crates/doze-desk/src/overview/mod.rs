//! A stack's overview page as plain data (Go `internal/desk/view`: `home.go`, `stopped.go`, `configerror.go`,
//! `grid.go`, `refs.go`): the head, the strip of figures, every instance under its group, the Grid, and for a stopped
//! stack the same rows read from its doze.hcl, each down. No GPUI here: the app draws these, and the goldens hold them
//! to the Go app's view JSON (`tests/pages`), field for field.
//!
//! Every word is the words' (`doze-display`); this module only puts them where the page shows them.

mod config;
mod grid;
mod home;
mod pane;
pub mod refs;
mod shape;
mod stopped;
mod vitals;

pub use config::{ConfigChanged, ConfigError, SrcLine, config_changed, config_error};
pub use grid::{GridCell, GridCol, GridHead, GridRow, GridTip, GridView, grid};
pub use home::{Group, Home, Row, home};
pub use pane::{Crash, Kv, NeedChip, Pane, Pill, pane};
pub use refs::{Block, HclFile, Refs, refs_in, scan};
pub use shape::{Decl, Shape, ShapeError, parse_status};
pub use stopped::{Stopped, stopped_home};
pub use vitals::{Conns, Meter, Share, States, Vitals4};

use doze_display as words;
use serde::Serialize;

/// How an instance's state reads: the word, the glyph and the tone it is drawn in (Go `view.Look`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Look {
    pub word: &'static str,
    pub glyph: &'static str,
    pub tone: &'static str,
}

impl Look {
    /// The words' look.
    pub fn of(l: words::Look) -> Look {
        Look {
            word: l.word(),
            glyph: l.glyph(),
            tone: l.tone().as_str(),
        }
    }

    /// A row of a stopped stack: nothing runs.
    pub const STOPPED: Look = Look {
        word: "stopped",
        glyph: "·",
        tone: "dim",
    };

    /// A row paused in doze.hcl (`enabled = false`).
    pub const PAUSED: Look = Look {
        word: "paused",
        glyph: "⊘",
        tone: "dim",
    };
}

impl Default for Look {
    fn default() -> Look {
        Look::STOPPED
    }
}

/// What the page says when what it shows cannot be believed (Go `view.Banner`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Banner {
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// Names joined as a sentence does: "api", "api and web", "api, web and db" (Go `andList`).
pub fn and_list<S: AsRef<str>>(names: &[S]) -> String {
    match names {
        [] => String::new(),
        [one] => one.as_ref().to_owned(),
        [init @ .., last] => {
            let head: Vec<&str> = init.iter().map(AsRef::as_ref).collect();
            format!("{} and {}", head.join(", "), last.as_ref())
        }
    }
}

/// A share of a sum, to three places, for a bar's width (Go `part`).
pub(crate) fn part(v: f64, sum: f64) -> f64 {
    if sum <= 0.0 {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)] // WHY: Go's int(v/sum*1000+0.5): a share, 0 to 1000
    let thousandths = (v / sum * 1000.0 + 0.5) as i64;
    #[allow(clippy::cast_precision_loss)] // WHY: at most 1000
    let out = thousandths as f64 / 1000.0;
    out
}

#[cfg(test)]
mod tests {
    use super::{and_list, part};

    #[test]
    fn lists_read_as_sentences() {
        assert_eq!(and_list::<&str>(&[]), "");
        assert_eq!(and_list(&["api"]), "api");
        assert_eq!(and_list(&["api", "web"]), "api and web");
        assert_eq!(and_list(&["api", "web", "db"]), "api, web and db");
    }

    #[test]
    fn parts_round_to_thousandths() {
        assert!((part(1.0, 3.0) - 0.333).abs() < 1e-9);
        assert!(part(1.0, 0.0).abs() < 1e-9);
    }
}
