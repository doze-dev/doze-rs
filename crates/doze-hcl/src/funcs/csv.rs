//! RFC 4180 records as Go's `encoding/csv` reads them with its defaults (comma, no comments, strict
//! quotes): empty lines skipped, `\r\n` read as `\n`, quoted fields that may span lines with `""` for
//! a quote, and every record as long as the first.

/// Why a record could not be read, with the line Go's `ParseError` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CsvError {
    pub(super) line: usize,
    pub(super) msg: &'static str,
}

const BARE_QUOTE: &str = "bare \" in non-quoted-field";
const QUOTE: &str = "extraneous or missing \" in quoted-field";
const FIELD_COUNT: &str = "wrong number of fields";

pub(super) struct Reader<'a> {
    rest: &'a str,
    line: usize,
    fields: Option<usize>,
}

impl<'a> Reader<'a> {
    pub(super) fn new(text: &'a str) -> Reader<'a> {
        Reader {
            rest: text,
            line: 0,
            fields: None,
        }
    }

    /// One line with its `\n` (a final `\r` at the end of the input dropped, `\r\n` made `\n`).
    fn read_line(&mut self) -> Option<String> {
        if self.rest.is_empty() {
            return None;
        }
        let end = self.rest.find('\n').map_or(self.rest.len(), |i| i + 1);
        let (line, rest) = self.rest.split_at(end);
        self.rest = rest;
        self.line += 1;
        let mut line = line.to_owned();
        if rest.is_empty() && !line.ends_with('\n') && line.ends_with('\r') {
            line.pop();
        }
        if line.ends_with("\r\n") {
            line.truncate(line.len() - 2);
            line.push('\n');
        }
        Some(line)
    }

    /// The next record, `None` at the end of the input.
    pub(super) fn read(&mut self) -> Option<Result<Vec<String>, CsvError>> {
        let mut line = loop {
            let l = self.read_line()?;
            if l != "\n" && !l.is_empty() {
                break l;
            }
        };
        let rec_line = self.line;
        let mut fields = Vec::new();
        loop {
            if !line.starts_with('"') {
                let (field, more) = match line.find(',') {
                    Some(i) => (line.get(..i).unwrap_or(""), true),
                    None => (line.strip_suffix('\n').unwrap_or(&line), false),
                };
                if field.contains('"') {
                    return Some(Err(self.error(BARE_QUOTE)));
                }
                fields.push(field.to_owned());
                if !more {
                    break;
                }
                line = line.get(field.len() + 1..).unwrap_or("").to_owned();
                continue;
            }
            match self.quoted(line.get(1..).unwrap_or("")) {
                Ok((field, rest)) => {
                    fields.push(field);
                    match rest {
                        Some(r) => line = r,
                        None => break,
                    }
                }
                Err(e) => return Some(Err(e)),
            }
        }
        Some(self.check_count(fields, rec_line))
    }

    /// A quoted field (after its opening quote): the field, and the rest of the line after the comma
    /// that ends it (`None` when the record ends).
    fn quoted(&mut self, start: &str) -> Result<(String, Option<String>), CsvError> {
        let mut field = String::new();
        let mut line = start.to_owned();
        loop {
            if let Some(i) = line.find('"') {
                field.push_str(line.get(..i).unwrap_or(""));
                let after = line.get(i + 1..).unwrap_or("").to_owned();
                match after.chars().next() {
                    Some('"') => {
                        field.push('"');
                        line = after.get(1..).unwrap_or("").to_owned();
                    }
                    Some(',') => return Ok((field, Some(after.get(1..).unwrap_or("").to_owned()))),
                    None | Some('\n') if after.len() <= 1 => return Ok((field, None)),
                    _ => return Err(self.error(QUOTE)),
                }
            } else if !line.is_empty() {
                field.push_str(&line);
                match self.read_line() {
                    Some(next) => line = next,
                    None => return Err(self.error(QUOTE)),
                }
            } else {
                return Err(self.error(QUOTE));
            }
        }
    }

    fn check_count(
        &mut self,
        fields: Vec<String>,
        rec_line: usize,
    ) -> Result<Vec<String>, CsvError> {
        match self.fields {
            None => self.fields = Some(fields.len()),
            Some(n) if n != fields.len() => {
                return Err(CsvError {
                    line: rec_line,
                    msg: FIELD_COUNT,
                });
            }
            Some(_) => {}
        }
        Ok(fields)
    }

    fn error(&self, msg: &'static str) -> CsvError {
        CsvError {
            line: self.line,
            msg,
        }
    }
}
