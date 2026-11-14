//! What a program prints is not ours to trust on a screen. A log line can carry colour codes, a window-title
//! sequence, a bell, a carriage return a progress bar uses to overwrite itself, or bytes that are not text, and any
//! of them, drawn raw, corrupts a terminal or injects into a page. Every string from outside is made plain text
//! before it is kept. Text that is already plain is handed back as it is, without a copy.

use std::borrow::Cow;

use crate::journal::Event;
use crate::model::Instance;

// fuzz:parser fuzz_clean
/// `s` as one line of printable text: escape sequences removed, the text before the last carriage return dropped
/// (what a progress bar overwrote), tabs as four spaces, a newline as a space, other control characters and the
/// marks that move text about (zero-width, byte-order and direction marks) gone. (Go: `Clean`.)
#[must_use]
pub fn clean(s: &str) -> Cow<'_, str> {
    clean_bytes(s.as_bytes())
}

/// [`clean`] for bytes that may not be UTF-8: each run of bytes that is not becomes one `U+FFFD`.
#[must_use]
pub fn clean_bytes(s: &[u8]) -> Cow<'_, str> {
    if s.contains(&0x1b) {
        let stripped = crate::ansi::strip(s);
        return Cow::Owned(plain(after_last_cr(&stripped)).into_owned());
    }
    plain(after_last_cr(s))
}

/// The text after the last carriage return, unless only newlines follow it; then the text before it.
fn after_last_cr(s: &[u8]) -> &[u8] {
    let Some(i) = s.iter().rposition(|&b| b == b'\r') else {
        return s;
    };
    let (before, after) = (
        s.get(..i).unwrap_or_default(),
        s.get(i + 1..).unwrap_or_default(),
    );
    let end = after.iter().rposition(|&b| b != b'\n').map_or(0, |e| e + 1);
    match after.get(..end) {
        Some(rest) if !rest.is_empty() => rest,
        _ => before,
    }
}

/// Valid UTF-8 with the characters that do not draw taken out.
fn plain(s: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(s) {
        Ok(text) if !text.chars().any(changes) => Cow::Borrowed(text),
        Ok(text) => Cow::Owned(filter(text)),
        Err(_) => Cow::Owned(filter(&valid_utf8(s))),
    }
}

/// Each run of invalid bytes as one replacement character, as Go's `strings.ToValidUTF8` does.
fn valid_utf8(s: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_bad = false;
    for chunk in s.utf8_chunks() {
        if !chunk.valid().is_empty() {
            out.push_str(chunk.valid());
            last_bad = false;
        }
        if !chunk.invalid().is_empty() && !last_bad {
            out.push('\u{FFFD}');
            last_bad = true;
        }
    }
    out
}

fn changes(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\0'..='\x1f' | '\x7f'..='\u{9f}' | '\u{200b}' | '\u{feff}' | '\u{202e}' | '\u{2066}' | '\u{2067}' | '\u{2069}')
}

fn filter(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            '\n' => out.push(' '),
            c if changes(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// [`clean`] for text of several lines, which it keeps. (Go: `CleanLines`.)
#[must_use]
pub fn clean_lines(s: &str) -> Cow<'_, str> {
    let cleaned: Vec<Cow<'_, str>> = s.split('\n').map(clean).collect();
    if cleaned
        .iter()
        .zip(s.split('\n'))
        .all(|(c, line)| matches!(c, Cow::Borrowed(b) if b.len() == line.len()))
    {
        return Cow::Borrowed(s);
    }
    Cow::Owned(cleaned.join("\n"))
}

/// Replaces `s` with its cleaned text when that differs.
fn clean_in_place(s: &mut String, how: fn(&str) -> Cow<'_, str>) {
    let cleaned = match how(s) {
        Cow::Owned(o) => o,
        // WHY: a borrowed answer shorter than `s` is a slice of it (what follows a carriage return); keeping it
        // needs its own string, and this is the rare path
        Cow::Borrowed(b) if b.len() != s.len() => b.to_owned(),
        Cow::Borrowed(_) => return,
    };
    *s = cleaned;
}

/// Makes the text an instance carries safe to draw: its last error (lines kept), its name, command, URL, domain,
/// and why its task last ran. (Go: `CleanView`.)
pub fn clean_instance(v: &mut Instance) {
    clean_in_place(&mut v.last_error, clean_lines);
    if let Some(task) = v.task.as_mut() {
        clean_in_place(&mut task.why, clean);
    }
    for s in [&mut v.name, &mut v.run, &mut v.url, &mut v.domain] {
        clean_in_place(s, clean);
    }
}

/// Makes a journal event's detail safe to draw. (Go: `CleanEvent`.)
pub fn clean_event(e: &mut Event) {
    clean_in_place(&mut e.detail, clean);
}
