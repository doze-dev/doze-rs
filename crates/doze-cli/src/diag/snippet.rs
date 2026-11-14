//! A span in a file, drawn the way rustc and cargo made familiar: the location, the source line, a caret under the
//! offending text and a label beside it.

use serde::Serialize;

/// A place in a source file: what `--json` reports and what the renderer draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    /// The file, as the reader would type it (relative to where they are when it can be).
    pub file: String,
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    /// How many characters the caret covers (at least one).
    #[serde(skip)]
    pub len: usize,
    /// The source line itself.
    #[serde(skip)]
    pub text: String,
    /// What the caret says.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl Span {
    /// The span of bytes `start..end` of `source` (clamped to it and to character boundaries).
    pub fn of(file: &str, source: &str, start: usize, end: usize) -> Span {
        let start = floor_boundary(source, start.min(source.len()));
        let end = floor_boundary(source, end.clamp(start, source.len()));
        let line_start = source
            .get(..start)
            .and_then(|s| s.rfind('\n'))
            .map_or(0, |i| i + 1);
        let line_end = source
            .get(start..)
            .and_then(|s| s.find('\n'))
            .map_or(source.len(), |i| start + i);
        let before = source.get(line_start..start).unwrap_or_default();
        let covered = source.get(start..end.min(line_end)).unwrap_or_default();
        Span {
            file: file.to_string(),
            line: source.get(..start).map_or(0, |s| s.matches('\n').count()) + 1,
            column: before.chars().count() + 1,
            len: covered.chars().count().max(1),
            text: source
                .get(line_start..line_end)
                .unwrap_or_default()
                .trim_end_matches('\r')
                .to_string(),
            label: String::new(),
        }
    }

    /// The same span with a label beside the caret.
    pub fn labelled(mut self, label: impl Into<String>) -> Span {
        self.label = label.into();
        self
    }

    /// The lines that draw it: location, gutter, source, caret. `notes` go under the caret as `= …`.
    pub(crate) fn draw(&self, notes: &[String]) -> Vec<String> {
        let num = self.line.to_string();
        let gutter = " ".repeat(num.len() + 2);
        // A tab draws as four spaces, so the caret has to move with it.
        let text = self.text.replace('\t', "    ");
        let before: String = self
            .text
            .chars()
            .take(self.column.saturating_sub(1))
            .collect();
        let col = before.replace('\t', "    ").chars().count();
        let mut caret = format!("{gutter}| {}{}", " ".repeat(col), "^".repeat(self.len));
        if !self.label.is_empty() {
            caret.push(' ');
            caret.push_str(&self.label);
        }
        let mut out = vec![
            format!(
                "{}--> {}:{}:{}",
                " ".repeat(num.len() + 1),
                self.file,
                self.line,
                self.column
            ),
            format!("{gutter}|"),
            format!(" {num} | {text}"),
            caret,
        ];
        if !notes.is_empty() {
            out.push(format!("{gutter}|"));
            out.extend(notes.iter().map(|n| format!("{gutter}= {n}")));
        }
        out
    }
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The proposal's own example, character for character.
    #[test]
    fn draws_the_proposal_example() {
        let src = "valkey \"cache\" {\n  version = 8\n\n  maxmemory_policy = \"allkeys-lur\"\n}\n";
        let at = src.find("\"allkeys-lur\"").unwrap();
        let span = Span::of("doze.hcl", src, at, at + 13).labelled("did you mean \"allkeys-lru\"?");
        let drawn = span.draw(&["valkey refuses this when it starts; doze checks it now".into()]);
        assert_eq!(
            drawn.join("\n"),
            "  --> doze.hcl:4:22\n   |\n 4 |   maxmemory_policy = \"allkeys-lur\"\n   |                      ^^^^^^^^^^^^^ did you mean \"allkeys-lru\"?\n   |\n   = valkey refuses this when it starts; doze checks it now"
        );
    }

    /// Spans from a parser can land anywhere: past the end, inside a character, across lines.
    #[test]
    fn any_span_draws_without_a_panic() {
        let src = "é\tx\nsecond";
        for (s, e) in [(0, 0), (1, 2), (3, 100), (100, 1), (4, 9)] {
            let span = Span::of("f", src, s, e);
            assert!(span.line >= 1 && span.column >= 1 && span.len >= 1);
            let _ = span.draw(&[]);
        }
        assert_eq!(Span::of("f", src, 6, 7).line, 2);
    }
}
