//! Colour for a person at a terminal, and none otherwise: only when the stream is a terminal, `NO_COLOR` is unset or empty, and
//! `TERM` is not `dumb`.

use std::io::IsTerminal;

/// How text is marked on one stream.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Style {
    on: bool,
}

/// The stream.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Stream {
    Out,
}

impl Style {
    /// The style for `stream`.
    pub(crate) fn of(stream: Stream) -> Self {
        let tty = match stream {
            Stream::Out => std::io::stdout().is_terminal(),
        };
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        Self {
            on: tty && !no_color && !dumb,
        }
    }

    /// No marks at all.
    #[cfg(test)]
    pub(crate) const fn plain() -> Self {
        Self { on: false }
    }

    fn wrap(self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    pub(crate) fn bold(self, s: &str) -> String {
        self.wrap("1", s)
    }

    pub(crate) fn dim(self, s: &str) -> String {
        self.wrap("2", s)
    }

    pub(crate) fn green(self, s: &str) -> String {
        self.wrap("32", s)
    }

    pub(crate) fn red(self, s: &str) -> String {
        self.wrap("31", s)
    }

    pub(crate) fn yellow(self, s: &str) -> String {
        self.wrap("33", s)
    }

    pub(crate) fn cyan(self, s: &str) -> String {
        self.wrap("36", s)
    }

    /// The mark of a verdict: ok, warn or fail.
    pub(crate) fn mark(self, verdict: Verdict) -> String {
        match verdict {
            Verdict::Ok => self.green("ok"),
            Verdict::Warn => self.yellow("!!"),
            Verdict::Fail => self.red("no"),
        }
    }
}

/// A check's outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ok,
    Warn,
    Fail,
}

/// `s` padded with spaces to `width` characters (a styled string pads by its visible text: pad before styling).
pub(crate) fn pad(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}
