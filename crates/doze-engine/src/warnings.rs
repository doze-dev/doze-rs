//! The engine's warnings, in its host's log. The engine and the module host say what went wrong through `tracing` (a
//! process killed because it would not stop, a stop command that failed, a shutdown that ran out of time), and `doze`
//! installed no subscriber: those lines went nowhere, so a `down` that killed a valkey before it could save looked, in
//! every log, like a clean stop. [`install`] (the stack daemon calls it) prints warnings and errors to stderr, which is
//! the daemon's log file, stamped, and nothing else; it holds no spans and costs no dependency beyond `tracing`.

use std::fmt::Write as _;
use std::io::Write as _;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// Makes [`Warnings`] the process's subscriber; one already set (a test's) is kept.
pub fn install() {
    let _ = tracing::subscriber::set_global_default(Warnings);
}

/// Warnings and errors to stderr, one line each.
#[derive(Debug)]
pub struct Warnings;

impl Subscriber for Warnings {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        *m.level() <= Level::WARN
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::WARN)
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, e: &Event<'_>) {
        // One write, and no panic when it fails (a full disk): `eprintln!` would panic.
        let line = format!("{}\n", line(e.metadata().level(), e));
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// `doze daemon: <stamp> warning: <message> key=value…`.
fn line(level: &Level, e: &Event<'_>) -> String {
    let mut text = Text::default();
    e.record(&mut text);
    let word = if *level == Level::ERROR {
        "error"
    } else {
        "warning"
    };
    let stamp = doze_logstore::stamp(std::time::SystemTime::now());
    format!("doze daemon: {stamp} {word}: {}{}", text.message, text.rest)
}

/// An event's message, and its other fields after it.
#[derive(Default)]
struct Text {
    message: String,
    rest: String,
}

impl Visit for Text {
    fn record_str(&mut self, f: &Field, v: &str) {
        if f.name() == "message" {
            self.message.push_str(v);
        } else {
            let _ = write!(self.rest, " {}={v}", f.name());
        }
    }

    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        if f.name() == "message" {
            let _ = write!(self.message, "{v:?}");
        } else {
            let _ = write!(self.rest, " {}={v:?}", f.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// What [`line`] makes of the events it is given.
    struct Lines(Arc<Mutex<Vec<String>>>);

    impl Subscriber for Lines {
        fn enabled(&self, m: &Metadata<'_>) -> bool {
            Warnings.enabled(m)
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, e: &Event<'_>) {
            if let Ok(mut l) = self.0.lock() {
                l.push(line(e.metadata().level(), e));
            }
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    #[test]
    fn warnings_and_errors_are_lines_and_the_rest_is_not() {
        let got = Arc::new(Mutex::new(Vec::new()));
        tracing::subscriber::with_default(Lines(got.clone()), || {
            tracing::info!("reaping");
            tracing::warn!(
                "\"cache\" did not exit within 8s of being asked to stop; it was killed (SIGKILL)"
            );
            tracing::error!(pid = 7, "gone");
        });
        let got = got.lock().map(|l| l.clone()).unwrap_or_default();
        assert_eq!(got.len(), 2, "{got:?}");
        let first = got.first().cloned().unwrap_or_default();
        assert!(first.starts_with("doze daemon: 20"), "{first}");
        assert!(
            first.ends_with(" warning: \"cache\" did not exit within 8s of being asked to stop; it was killed (SIGKILL)"),
            "{first}"
        );
        let second = got.get(1).cloned().unwrap_or_default();
        assert!(second.ends_with(" error: gone pid=7"), "{second}");
    }
}
