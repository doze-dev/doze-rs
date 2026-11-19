//! The words of a metric math expression: numbers, quoted strings, names, brackets and operators. The operators are those AWS's
//! metric math syntax lists (`+ - * / ^`, the comparisons `== != <= >= < >`, and `AND`/`&&`, `OR`/`||`, `NOT`) (memory: the user
//! guide's "Metric math syntax and functions" page, not recorded).

/// One word of an expression.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Open,
    Close,
    OpenList,
    CloseList,
    Comma,
    Op(Op),
}

/// An operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Not,
}

/// The longest expression read (the model's `MetricExpression`: at most 2,048 characters).
pub(crate) const MAX_LEN: usize = 2048;

/// Splits `text` into words.
///
/// # Errors
/// What is wrong, in a sentence.
pub(crate) fn lex(text: &str) -> Result<Vec<Tok>, String> {
    if text.len() > MAX_LEN {
        return Err(format!(
            "the expression is longer than {MAX_LEN} characters"
        ));
    }
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((at, c)) = it.next() {
        let tok = match c {
            c if c.is_whitespace() => continue,
            '(' => Tok::Open,
            ')' => Tok::Close,
            '[' => Tok::OpenList,
            ']' => Tok::CloseList,
            ',' => Tok::Comma,
            '+' => Tok::Op(Op::Add),
            '-' => Tok::Op(Op::Sub),
            '*' => Tok::Op(Op::Mul),
            '/' => Tok::Op(Op::Div),
            '^' => Tok::Op(Op::Pow),
            '<' | '>' | '=' | '!' | '&' | '|' => {
                let next = it.peek().map(|(_, n)| *n);
                let (op, two) = pair(c, next).ok_or_else(|| format!("unexpected '{c}' at {at}"))?;
                if two {
                    it.next();
                }
                Tok::Op(op)
            }
            '"' | '\'' => Tok::Str(quoted(&mut it, c)?),
            c if c.is_ascii_digit() || c == '.' => Tok::Num(number(text, at, &mut it)?),
            c if c.is_ascii_alphabetic() || c == '_' => name(text, at, &mut it),
            other => return Err(format!("unexpected '{other}' at {at}")),
        };
        out.push(tok);
    }
    Ok(out)
}

/// An operator of one or two characters starting with `c`; `true` when it took the second.
fn pair(c: char, next: Option<char>) -> Option<(Op, bool)> {
    Some(match (c, next) {
        ('<', Some('=')) => (Op::Le, true),
        ('>', Some('=')) => (Op::Ge, true),
        ('=', Some('=')) => (Op::Eq, true),
        ('!', Some('=')) => (Op::Ne, true),
        ('&', Some('&')) => (Op::And, true),
        ('|', Some('|')) => (Op::Or, true),
        ('<', _) => (Op::Lt, false),
        ('>', _) => (Op::Gt, false),
        ('!', _) => (Op::Not, false),
        _ => return None,
    })
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn quoted(it: &mut Chars<'_>, quote: char) -> Result<String, String> {
    let mut s = String::new();
    loop {
        match it.next() {
            None => return Err("a string is not closed".to_owned()),
            Some((_, '\\')) => match it.next() {
                Some((_, c)) => s.push(c),
                None => return Err("a string is not closed".to_owned()),
            },
            Some((_, c)) if c == quote => return Ok(s),
            Some((_, c)) => s.push(c),
        }
    }
}

fn number(text: &str, start: usize, it: &mut Chars<'_>) -> Result<f64, String> {
    let mut end = start + 1;
    let mut prev = text.get(start..end).unwrap_or_default().chars().next();
    while let Some((at, c)) = it.peek().copied() {
        let exponent_sign = matches!(prev, Some('e' | 'E')) && (c == '+' || c == '-');
        if !(c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || exponent_sign) {
            break;
        }
        it.next();
        end = at + c.len_utf8();
        prev = Some(c);
    }
    let word = text.get(start..end).unwrap_or_default();
    word.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("'{word}' is not a number"))
}

fn name(text: &str, start: usize, it: &mut Chars<'_>) -> Tok {
    let mut end = start + 1;
    while let Some((at, c)) = it.peek().copied() {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            break;
        }
        it.next();
        end = at + 1;
    }
    let word = text.get(start..end).unwrap_or_default();
    match word {
        "AND" => Tok::Op(Op::And),
        "OR" => Tok::Op(Op::Or),
        "NOT" => Tok::Op(Op::Not),
        _ => Tok::Name(word.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_split_as_the_syntax_has_them() {
        let t = lex("m1 * 2.5e1 >= m2 AND 'x' || NOT[a]").unwrap();
        assert_eq!(t.len(), 12);
        assert_eq!(t.get(2), Some(&Tok::Num(25.0)));
        assert_eq!(t.get(3), Some(&Tok::Op(Op::Ge)));
        assert_eq!(t.get(5), Some(&Tok::Op(Op::And)));
        assert_eq!(t.get(6), Some(&Tok::Str("x".to_owned())));
        assert!(lex("a = b").is_err());
        assert!(lex("'open").is_err());
        assert!(lex("1.2.3").is_err());
        assert!(lex("a # b").is_err());
    }
}
