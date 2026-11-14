//! The daemon's wire types become the words' inputs in `doze-control` (feature `words`), shared with the CLI, so the
//! app and the terminal cannot read an instance two ways. Re-exported here under the name the app has always used.

pub use doze_control::words::*;
