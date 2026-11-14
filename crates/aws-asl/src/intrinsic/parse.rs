//! Reading an intrinsic call: `States.Format('{} and {}', $.a, States.ArrayLength($.b))`.

use serde_json::Value;

use crate::path::Path;

/// One argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Literal(Value),
    Path(Path),
    Call(Call),
}

/// A call of a `States.` function.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: String,
    pub args: Vec<Arg>,
}

// fuzz:parser fuzz_intrinsic
/// Reads `text` as one intrinsic call.
///
/// # Errors
/// What is wrong with it.
pub fn parse(text: &str) -> Result<Call, String> {
    let mut p = Parser { text, at: 0 };
    let call = p.call(0)?;
    p.blank();
    if p.at < text.len() {
        return Err(format!("unexpected text after the call: {}", p.rest()));
    }
    Ok(call)
}

/// How deep calls may nest.
const DEEPEST: usize = 16;

struct Parser<'a> {
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn blank(&mut self) {
        let skip = self.rest().len() - self.rest().trim_start().len();
        self.at += skip;
    }

    fn eat(&mut self, s: &str) -> bool {
        self.blank();
        if self.rest().starts_with(s) {
            self.at += s.len();
            true
        } else {
            false
        }
    }

    fn call(&mut self, depth: usize) -> Result<Call, String> {
        if depth > DEEPEST {
            return Err("intrinsic calls are nested too deeply".to_owned());
        }
        self.blank();
        if !self.rest().starts_with("States.") {
            return Err("an intrinsic function's name must start with States.".to_owned());
        }
        let name: String = self
            .rest()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        self.at += name.len();
        if !self.eat("(") {
            return Err(format!("{name} must be followed by its arguments in ( )"));
        }
        let mut args = Vec::new();
        if !self.eat(")") {
            loop {
                args.push(self.arg(depth)?);
                if self.eat(")") {
                    break;
                }
                if !self.eat(",") {
                    return Err(format!(
                        "the arguments of {name} are not separated by commas"
                    ));
                }
            }
        }
        Ok(Call { name, args })
    }

    fn arg(&mut self, depth: usize) -> Result<Arg, String> {
        self.blank();
        let rest = self.rest();
        if rest.starts_with("States.") {
            return Ok(Arg::Call(self.call(depth + 1)?));
        }
        if rest.starts_with('\'') {
            return Ok(Arg::Literal(Value::String(self.string()?)));
        }
        if rest.starts_with('$') {
            let end = path_end(rest);
            let text = rest.get(..end).unwrap_or_default().trim_end();
            let path = Path::parse(text)?;
            self.at += end;
            return Ok(Arg::Path(path));
        }
        let word: String = rest
            .chars()
            .take_while(|c| !matches!(c, ',' | ')') && !c.is_whitespace())
            .collect();
        self.at += word.len();
        serde_json::from_str::<Value>(&word)
            .ok()
            .filter(|v| !v.is_object() && !v.is_array())
            .map(Arg::Literal)
            .ok_or_else(|| format!("'{word}' is not an argument an intrinsic function takes"))
    }

    /// A single-quoted string; `\'`, `\{`, `\}` and `\\` are escapes.
    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\'' => {
                    self.at += i + 1;
                    return Ok(out);
                }
                '\\' => match chars.next() {
                    // `\{` and `\}` stay escaped: States.Format reads them.
                    Some((_, e @ ('{' | '}'))) => {
                        out.push('\\');
                        out.push(e);
                    }
                    Some((_, e)) => out.push(e),
                    None => break,
                },
                c => out.push(c),
            }
        }
        Err("a string argument is not closed".to_owned())
    }
}

/// Where a path argument ends: the first `,` or `)` outside brackets and quotes.
fn path_end(text: &str) -> usize {
    let (mut depth, mut quote) = (0i32, None);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '[' | '(') => depth += 1,
            (None, ']') => depth -= 1,
            (None, ')') if depth > 0 => depth -= 1,
            (None, ',' | ')') if depth == 0 => return i,
            _ => {}
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_read() {
        let c = parse("States.Format('{}-{}', $.a, States.ArrayLength($.b[0]), 3, true)").unwrap();
        assert_eq!(c.name, "States.Format");
        assert_eq!(c.args.len(), 5);
        assert!(matches!(&c.args[2], Arg::Call(inner) if inner.name == "States.ArrayLength"));
        assert_eq!(c.args[3], Arg::Literal(serde_json::json!(3)));
        assert_eq!(parse("States.UUID()").unwrap().args.len(), 0);
        assert_eq!(
            parse(r"States.Format('it\'s {}', $.a)").unwrap().args[0],
            Arg::Literal(serde_json::json!("it's {}"))
        );
        assert!(parse("States.Format('x'").is_err());
        assert!(parse("Format('x')").is_err());
        assert!(parse("States.Array($.a['x,y'], 1)").is_ok());
    }
}
