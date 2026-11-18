//! Colour, and only on a terminal: off when the stream is piped, under `NO_COLOR` (any value, no-color.org) and
//! under `--no-color`. The words are the same either way; colour only adds emphasis (rule 6).

use std::borrow::Cow;

/// The styles the CLI uses. Few on purpose: each means one thing everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    /// Done, verified (a tick).
    Ok,
    /// Something to look at that is not a failure (`!`).
    Warn,
    /// A failure (`error:`, a cross).
    Fail,
    /// Names, headings.
    Bold,
    /// Secondary detail.
    Dim,
    /// Commands the reader can type.
    Cmd,
}

impl Style {
    fn sgr(self) -> &'static str {
        match self {
            Style::Ok => "32",
            Style::Warn => "33",
            Style::Fail => "1;31",
            Style::Bold => "1",
            Style::Dim => "2",
            Style::Cmd => "36",
        }
    }
}

/// Paints text for one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Paint {
    pub(crate) color: bool,
}

impl Paint {
    pub(crate) fn new(color: bool) -> Paint {
        Paint { color }
    }

    /// `text` in `style`, or `text` itself without colour.
    pub(crate) fn style<'a>(self, style: Style, text: &'a str) -> Cow<'a, str> {
        if self.color && !text.is_empty() {
            Cow::Owned(format!("\x1b[{}m{text}\x1b[0m", style.sgr()))
        } else {
            Cow::Borrowed(text)
        }
    }

    pub(crate) fn ok<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Ok, t)
    }
    pub(crate) fn warn<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Warn, t)
    }
    pub(crate) fn fail<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Fail, t)
    }
    pub(crate) fn bold<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Bold, t)
    }
    pub(crate) fn dim<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Dim, t)
    }
    pub(crate) fn cmd<'a>(self, t: &'a str) -> Cow<'a, str> {
        self.style(Style::Cmd, t)
    }
}

/// The width a string takes in a terminal column count, near enough for tables of names and versions.
pub(crate) fn width(s: &str) -> usize {
    s.chars().count()
}

/// `s` padded with spaces to `w` columns (never cut).
pub(crate) fn pad(s: &str, w: usize) -> String {
    let mut out = String::with_capacity(w.max(s.len()));
    out.push_str(s);
    out.extend(std::iter::repeat_n(' ', w.saturating_sub(width(s))));
    out
}
