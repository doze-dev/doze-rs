//! Scalars (plain, quoted, block) and flow collections.

use serde_json::{Number, Value};

use super::{P, R, blank};

impl P<'_> {
    /// When a plain mapping key starts here and ends on this line with `: `, where it ends.
    pub(super) fn plain_key_ahead(&self) -> Option<usize> {
        let mut j = self.i;
        if matches!(self.b.get(j), Some(b'#' | b'-' | b'?' | b':'))
            && !blank(self.b.get(j + 1).copied())
        {
            j += 1;
        } else if matches!(
            self.b.get(j),
            Some(
                b'#' | b'-'
                    | b'?'
                    | b':'
                    | b','
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'&'
                    | b'*'
                    | b'!'
                    | b'|'
                    | b'>'
                    | b'%'
                    | b'@'
                    | b'`'
            )
        ) {
            return None;
        }
        loop {
            match self.b.get(j) {
                None | Some(b'\n') => return None,
                Some(b':') if blank(self.b.get(j + 1).copied()) => return Some(j),
                Some(b'#') if matches!(self.b.get(j.wrapping_sub(1)), Some(b' ' | b'\t')) => {
                    return None;
                }
                _ => j += 1,
            }
        }
    }

    /// A plain scalar in block context, with its continuation lines (more indented than `parent`), resolved to its type.
    pub(super) fn plain_block(&mut self, parent: isize) -> R<Value> {
        if matches!(self.peek(), Some(b'@' | b'`' | b'%' | b',' | b']' | b'}')) {
            return self.fail();
        }
        let mut text = self.plain_line(false)?;
        loop {
            let save = self.i;
            let mut breaks = 0;
            // Look past the line break for a continuation line.
            while self.peek() == Some(b'\n') {
                self.i += 1;
                breaks += 1;
                self.skip_space();
            }
            let col = self.col() as isize;
            if breaks == 0
                || self.at_eol()
                || self.peek() == Some(b'#')
                || col <= parent
                || self.at_document_marker()
            {
                self.i = save;
                break;
            }
            if self.plain_key_ahead().is_some() {
                return self.fail();
            }
            let more = self.plain_line(false)?;
            if breaks == 1 {
                text.push(' ');
            } else {
                text.extend(std::iter::repeat_n('\n', breaks - 1));
            }
            text.push_str(&more);
        }
        self.end_of_line()?;
        Ok(resolve(&text))
    }

    /// One line of a plain scalar, up to a comment, the line end, or (in flow) an indicator.
    pub(super) fn plain_line(&mut self, flow: bool) -> R<String> {
        let start = self.i;
        loop {
            match self.peek() {
                None | Some(b'\n') => break,
                Some(b':')
                    if blank(self.peek_at(1))
                        || (flow
                            && matches!(
                                self.peek_at(1),
                                Some(b',' | b']' | b'}' | b'[' | b'{')
                            )) =>
                {
                    break;
                }
                Some(b'#') if matches!(self.b.get(self.i.wrapping_sub(1)), Some(b' ' | b'\t')) => {
                    break;
                }
                Some(b',' | b'[' | b']' | b'{' | b'}') if flow => break,
                _ => self.i += 1,
            }
        }
        let text = self.s.get(start..self.i).unwrap_or_default();
        Ok(text.trim_end_matches([' ', '\t']).to_owned())
    }

    /// A single- or double-quoted scalar.
    pub(super) fn quoted(&mut self) -> R<String> {
        let double = self.peek() == Some(b'"');
        self.i += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return self.fail();
            };
            match c {
                b'\'' if !double => {
                    if self.peek_at(1) == Some(b'\'') {
                        out.push('\'');
                        self.i += 2;
                    } else {
                        self.i += 1;
                        return Ok(out);
                    }
                }
                b'"' if double => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' if double => self.escape(&mut out)?,
                b'\n' => self.fold_quoted(&mut out),
                _ => {
                    let rest = self.s.get(self.i..).unwrap_or_default();
                    let ch = rest.chars().next().unwrap_or('\u{fffd}');
                    out.push(ch);
                    self.i += ch.len_utf8();
                }
            }
        }
    }

    /// A line break inside quotes: trailing spaces go, one break is a space, more are line feeds.
    fn fold_quoted(&mut self, out: &mut String) {
        let kept = out.trim_end_matches([' ', '\t']).len();
        out.truncate(kept);
        let mut breaks = 0;
        while self.peek() == Some(b'\n') {
            self.i += 1;
            breaks += 1;
            self.skip_space();
        }
        if breaks == 1 {
            out.push(' ');
        } else {
            out.extend(std::iter::repeat_n('\n', breaks - 1));
        }
    }

    fn escape(&mut self, out: &mut String) -> R<()> {
        let at = self.i;
        self.i += 1;
        let Some(c) = self.peek() else {
            return self.fail();
        };
        self.i += 1;
        let simple = match c {
            b'0' => Some('\0'),
            b'a' => Some('\u{7}'),
            b'b' => Some('\u{8}'),
            b't' | b'\t' => Some('\t'),
            b'n' => Some('\n'),
            b'v' => Some('\u{b}'),
            b'f' => Some('\u{c}'),
            b'r' => Some('\r'),
            b'e' => Some('\u{1b}'),
            b' ' => Some(' '),
            b'"' => Some('"'),
            b'/' => Some('/'),
            b'\\' => Some('\\'),
            b'N' => Some('\u{85}'),
            b'_' => Some('\u{a0}'),
            b'L' => Some('\u{2028}'),
            b'P' => Some('\u{2029}'),
            _ => None,
        };
        if let Some(ch) = simple {
            out.push(ch);
            return Ok(());
        }
        let digits = match c {
            b'x' => 2,
            b'u' => 4,
            b'U' => 8,
            b'\n' => {
                // An escaped line break joins the lines with nothing between.
                self.skip_space();
                while self.peek() == Some(b'\n') {
                    self.i += 1;
                    out.push('\n');
                    self.skip_space();
                }
                return Ok(());
            }
            _ => return Err(self.error_at(at)),
        };
        let hex = self
            .s
            .get(self.i..self.i + digits)
            .ok_or_else(|| self.error_at(at))?;
        let n = u32::from_str_radix(hex, 16).map_err(|_| self.error_at(at))?;
        out.push(char::from_u32(n).ok_or_else(|| self.error_at(at))?);
        self.i += digits;
        Ok(())
    }

    /// A literal (`|`) or folded (`>`) block scalar, with its chomping and indentation indicators.
    pub(super) fn block_scalar(&mut self, parent: isize) -> R<Value> {
        let folded = self.peek() == Some(b'>');
        self.i += 1;
        let (mut chomp, mut given) = (b' ', None);
        for _ in 0..2 {
            match self.peek() {
                Some(c @ (b'+' | b'-')) => {
                    chomp = c;
                    self.i += 1;
                }
                Some(c @ b'1'..=b'9') => {
                    given = Some(usize::from(c - b'0'));
                    self.i += 1;
                }
                _ => {}
            }
        }
        self.end_of_line()?;
        let base = usize::try_from(parent.max(0)).unwrap_or(0);
        let mut indent = given.map(|d| if parent < 0 { d } else { base + d });
        let mut lines: Vec<&str> = Vec::new();
        while self.peek() == Some(b'\n') {
            let line_start = self.i + 1;
            let end = self
                .s
                .get(line_start..)
                .and_then(|t| t.find('\n'))
                .map_or(self.s.len(), |e| line_start + e);
            let line = self.s.get(line_start..end).unwrap_or_default();
            let spaces = line.len() - line.trim_start_matches(' ').len();
            let empty = line.trim_start_matches(' ').is_empty();
            if !empty {
                let want = *indent.get_or_insert(spaces);
                if spaces < want
                    || (spaces as isize) <= parent
                    || (spaces == 0 && (line.starts_with("---") || line.starts_with("...")))
                {
                    break;
                }
            }
            lines.push(line);
            self.i = end;
        }
        let indent = indent.unwrap_or(0);
        // Trailing empty lines belong to the chomping, not the content.
        let mut trailing = 0;
        while lines
            .last()
            .is_some_and(|l| l.len() <= indent || l.trim_start_matches(' ').is_empty())
        {
            lines.pop();
            trailing += 1;
        }
        let body: Vec<&str> = lines
            .iter()
            .map(|l| l.get(indent..).unwrap_or_default())
            .collect();
        let mut text = if folded { fold(&body) } else { body.join("\n") };
        if !body.is_empty() {
            match chomp {
                b'-' => {}
                b'+' => text.extend(std::iter::repeat_n('\n', trailing + 1)),
                _ => text.push('\n'),
            }
        } else if chomp == b'+' {
            text.extend(std::iter::repeat_n('\n', trailing));
        }
        // The reader is left at the end of the last line it took (or of the indicator line).
        Ok(Value::String(text))
    }
}

/// Folds the lines of a `>` scalar: a single break between two plain lines is a space; lines indented further keep theirs.
fn fold(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut pending = 0usize;
    let mut prev_plain: Option<bool> = None;
    for line in lines {
        if line.is_empty() {
            pending += 1;
            continue;
        }
        let plain = !line.starts_with([' ', '\t']);
        match prev_plain {
            None => out.extend(std::iter::repeat_n('\n', pending)),
            Some(prev) if prev && plain => {
                if pending == 0 {
                    out.push(' ');
                } else {
                    out.extend(std::iter::repeat_n('\n', pending));
                }
            }
            Some(_) => out.extend(std::iter::repeat_n('\n', pending + 1)),
        }
        out.push_str(line);
        pending = 0;
        prev_plain = Some(plain);
    }
    out
}

/// A plain scalar's type, as YAML 1.1 resolves it (SnakeYAML): null, booleans (with `yes`, `no`, `on`, `off`), decimal integers
/// and floats. Anything else (octal-looking `0123`, an account id with a leading zero, a date) stays text.
pub(super) fn resolve(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Value::Bool(true);
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Value::Bool(false);
        }
        _ => {}
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    let decimal = !digits.is_empty()
        && digits.bytes().all(|c| c.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if decimal && let Ok(n) = text.trim_start_matches('+').parse::<i64>() {
        return Value::Number(n.into());
    }
    let float = digits.contains('.')
        && digits
            .bytes()
            .all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'-' | b'+'))
        && digits
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == b'.');
    if float
        && let Ok(f) = text.parse::<f64>()
        && let Some(n) = Number::from_f64(f)
        && n.to_string() == text.trim_start_matches('+')
    {
        // Only a float that prints back as written is a number; `1.10` stays the text it was.
        return Value::Number(n);
    }
    Value::String(text.to_owned())
}
