//! Acting (docs/port/app/PLAN.md M2 steps 2.1, 2.2, 2.5, 2.6): the confirmations and how they are answered, what is
//! said and kept, a whole stack's actions through the CLI, the checks before Up and before a service wakes, the start
//! sheet, and a folder with no doze.hcl yet. All of it plain data and plain functions; the app draws it and runs it
//! off the UI thread. Service verbs themselves go through [`crate::Hub::verb`].

mod asks;
mod choose;
mod detect;
mod dialog;
mod empty;
mod failure;
mod notice;
mod preflight;
mod remove;
mod stack;
mod starting;

pub use asks::{action_dialog, add_dialog, forget_dialog, stop_dialog};
pub use choose::{Choosing, Pressed};
pub use detect::{Detected, Found, compose_images, detect, make_targets};
pub use dialog::{Button, Dialog, Fact, Kv, Span, dialog_of};
pub use empty::{Empty, Way, empty_of, no_config, record, setup};
pub use failure::{Failure, LINES_KEPT, failure_of};
pub use notice::{BAD_FOR, GOOD_FOR, NOTICES_KEPT, Notice, Notices, TOASTS_SHOWN, Toast};
pub use preflight::{
    Fix, FixKind, Looked, PreCheck, Preflight, Tone, bindable, decl_port, listening, missing_files,
    parse_lsof_all, parse_ss_all, preflight, preflight_instance,
};
pub use remove::{STOP_WAIT, remove};
pub use stack::{Busy, Cli, Ran, StackAction, read_ran};
pub use starting::{Pick, StartSheet, StartWord, Starting, starting_set};
