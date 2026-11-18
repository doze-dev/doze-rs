//! The CLI's tables, drawn as Go's `ui.Table` draws them so `doze status` reads the same: a header, rows and label
//! lines; two spaces in front, three between columns, the last column never padded, trailing spaces trimmed. Widths
//! are the text's own, colour left out.

use crate::paint::{Paint, Style};

/// A table being filled.
#[derive(Debug, Default)]
pub(crate) struct Table {
    header: Vec<&'static str>,
    lines: Vec<Line>,
}

#[derive(Debug)]
enum Line {
    Row(Vec<String>),
    Label(String),
}

impl Table {
    pub(crate) fn new(header: &[&'static str]) -> Table {
        Table {
            header: header.to_vec(),
            lines: Vec::new(),
        }
    }

    /// A row of cells (already painted).
    pub(crate) fn row(&mut self, cells: Vec<String>) {
        self.lines.push(Line::Row(cells));
    }

    /// A heading line between rows, drawn dim.
    pub(crate) fn label(&mut self, text: &str) {
        self.lines.push(Line::Label(text.to_owned()));
    }

    /// The table, without a final newline.
    pub(crate) fn draw(&self, p: Paint) -> String {
        let mut widths: Vec<usize> = self.header.iter().map(|h| visible(h)).collect();
        for line in &self.lines {
            if let Line::Row(cells) = line {
                for (w, c) in widths.iter_mut().zip(cells) {
                    *w = (*w).max(visible(c));
                }
            }
        }
        let header: Vec<String> = self
            .header
            .iter()
            .map(|h| p.style(Style::Bold, h).into_owned())
            .collect();
        let mut out = row(&header, &widths);
        for line in &self.lines {
            out.push('\n');
            match line {
                Line::Row(cells) => out.push_str(&row(cells, &widths)),
                Line::Label(text) => out.push_str(&p.dim(text)),
            }
        }
        out
    }
}

fn row(cells: &[String], widths: &[usize]) -> String {
    let last = cells.len().saturating_sub(1);
    let parts: Vec<String> = cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i == last {
                return c.clone();
            }
            let w = widths.get(i).copied().unwrap_or(0);
            let mut s = c.clone();
            s.extend(std::iter::repeat_n(' ', w.saturating_sub(visible(c))));
            s
        })
        .collect();
    format!("  {}", parts.join("   ")).trim_end().to_owned()
}

/// The columns a painted string takes: its characters, escape sequences left out.
pub(crate) fn visible(s: &str) -> usize {
    let mut n = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ … final byte in @..~
            for d in chars.by_ref() {
                if ('@'..='~').contains(&d) && d != '[' {
                    break;
                }
            }
        } else {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_ignore_colour_and_the_last_column_is_not_padded() {
        let p = Paint::new(true);
        let mut t = Table::new(&["", "NAME", "NOTE"]);
        t.label("Modules");
        t.row(vec!["●".into(), "db".into(), "held".into()]);
        t.row(vec!["·".into(), "cache".into(), String::new()]);
        let plain = Table::draw(&t, Paint::new(false));
        assert_eq!(
            plain,
            "      NAME    NOTE\nModules\n  ●   db      held\n  ·   cache"
        );
        assert_eq!(visible(&p.ok("●")), 1);
        let mut coloured = Table::new(&["", "NAME"]);
        coloured.row(vec![p.ok("●").into_owned(), "db".into()]);
        assert_eq!(
            coloured.draw(Paint::new(false)),
            "      NAME
  \x1b[32m●\x1b[0m   db"
        );
    }
}
