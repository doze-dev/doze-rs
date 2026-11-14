//! The words doze uses on every surface: how an instance's state reads (its word, glyph and tone), what it is down
//! to, what it waits on and where to reach it; the counts, figures and times the screens show; the sentences of the
//! confirmations; a module console's cells; the palette's matching; and the cleaning of text from outside.
//!
//! A port of the Go package `internal/display`, which is its specification: the Rust app and the Rust CLI say what
//! the Go surfaces say, byte for byte (`tests/oracle.rs` holds the two to that over a generated input set). The
//! words are chosen here, once, so no two surfaces can disagree; each paints them in its own way.
//!
//! The data the words are about ([`Instance`], [`Snapshot`], [`Event`], [`ConsoleSpec`]) arrives as the daemon's
//! strings; the protocol crate parses each into these types once, so nothing here compares a state as text.

mod ansi;
mod banner;
mod cells;
mod clean;
mod console;
mod dialog;
mod empty;
mod format;
mod fuzzy;
mod gonum;
mod goparse;
mod gotime;
mod journal;
mod level;
mod look;
mod model;
mod note;
mod reach;
mod rows;
mod runs;
mod series;
mod tabs;
mod words;

pub use banner::{STALE_AFTER, banner, humanise, running_note, task_result, task_started};
pub use cells::{RateSample, action_key, cell, detail_rows, rates};
pub use clean::{clean, clean_bytes, clean_event, clean_instance, clean_lines};
pub use console::{
    CELL_RUNTIME, Column, ConsoleRow, ConsoleSpec, ConsoleTab, Node, NodeKind, Panel, PanelKind,
    Role,
};
pub use dialog::{
    Fact, Span, dependants, held_sentence, reset_command, reset_facts, restart_buttons,
    sleep_buttons,
};
pub use empty::{Choice, EMPTY_CHOICES, EMPTY_NOTHING_FOUND, STARTER_FILE};
pub use format::{ago, bytes_label, clock, count, home, mb, pct, plural, thousands, tilde};
pub use fuzzy::{fuzzy_match, fuzzy_score};
pub use gonum::parse_float;
pub use journal::{Cause, Event, EventKind, cause_words, sentence};
pub use level::{LogLevel, level, rank};
pub use look::{Look, Tone, awake_now, in_use, look, state_rank, switch_on, troubled};
pub use model::{Holder, Instance, Kind, Outcome, Snapshot, State, TaskRecord};
pub use note::{Said, first_line, note};
pub use reach::{
    Known, Need, Reach, address, address_of, connections, first_unmet, holder_named, mem, needs,
    reach, runs, short_run,
};
pub use rows::{Group, Row, SortBy, Tally, rows, tally, tree_order};
pub use runs::{Run, run_history};
pub use series::{
    Meter, SAMPLE_EVERY, Share, cores, cores_unit, mem_unit, scale, share_cells, shares, span_word,
    squeeze, squeeze_timed, stats,
};
pub use tabs::{
    TAB_LAST_RUN, TAB_LOGS, TAB_OVERVIEW, TAB_WEB, Tab, console_body, console_tabs, has_web, tabs,
    web_url,
};
pub use words::{because, hint, what};
