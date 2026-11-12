//! The tokens of every DynamoDB expression language.

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    /// A bare word: an attribute name, a function, a keyword.
    Word,
    /// `#name`.
    NameRef,
    /// `:value`.
    ValueRef,
    /// Digits, only meaningful inside `[ ]`.
    Int,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    /// A character no expression uses.
    Other,
    /// The end of the text.
    Eof,
}

/// A token and where it sits in the text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

fn word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Splits `text` into tokens, ending with [`Tok::Eof`].
pub(crate) fn tokens(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(&b) = bytes.get(at) {
        if b.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        let next = bytes.get(at + 1).copied();
        let (tok, len) = match b {
            b'#' | b':' => {
                let n = bytes
                    .get(at + 1..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|c| word_byte(**c))
                    .count();
                (
                    if n == 0 {
                        Tok::Other
                    } else if b == b'#' {
                        Tok::NameRef
                    } else {
                        Tok::ValueRef
                    },
                    1 + n,
                )
            }
            b'0'..=b'9' => (
                Tok::Int,
                bytes
                    .get(at..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count(),
            ),
            _ if b.is_ascii_alphabetic() || b == b'_' => (
                Tok::Word,
                bytes
                    .get(at..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(|c| word_byte(**c))
                    .count(),
            ),
            b'<' if next == Some(b'>') => (Tok::Ne, 2),
            b'<' if next == Some(b'=') => (Tok::Le, 2),
            b'>' if next == Some(b'=') => (Tok::Ge, 2),
            _ => (punct(b), char_len(text, at)),
        };
        at += len.max(1);
        out.push(Token {
            tok,
            start,
            end: at,
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        start: text.len(),
        end: text.len(),
    });
    out
}

fn punct(b: u8) -> Tok {
    match b {
        b'(' => Tok::LParen,
        b')' => Tok::RParen,
        b'[' => Tok::LBracket,
        b']' => Tok::RBracket,
        b',' => Tok::Comma,
        b'.' => Tok::Dot,
        b'=' => Tok::Eq,
        b'<' => Tok::Lt,
        b'>' => Tok::Gt,
        b'+' => Tok::Plus,
        b'-' => Tok::Minus,
        _ => Tok::Other,
    }
}

/// The length in bytes of the character at `at`, so a multi-byte character is one `Other` token.
fn char_len(text: &str, at: usize) -> usize {
    text.get(at..)
        .and_then(|s| s.chars().next())
        .map_or(1, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_the_pieces() {
        let kinds: Vec<Tok> = tokens("a.b[2] <> :v AND #n<=3")
            .iter()
            .map(|t| t.tok)
            .collect();
        use Tok::{Dot, Eof, Int, LBracket, Le, NameRef, Ne, RBracket, ValueRef, Word};
        assert_eq!(
            kinds,
            [
                Word, Dot, Word, LBracket, Int, RBracket, Ne, ValueRef, Word, NameRef, Le, Int, Eof
            ]
        );
    }

    #[test]
    fn a_stray_character_is_one_token() {
        let t = tokens("é");
        assert_eq!((t[0].tok, t[0].end), (Tok::Other, 2));
    }
}
