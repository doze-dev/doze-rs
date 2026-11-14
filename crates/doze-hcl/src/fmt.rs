//! `doze fmt`: hclwrite's `Format`, byte for byte (owner decision O11). Formatting works on Go's
//! token stream, not on a tree, and changes only the spaces before each token: indentation from the
//! brackets each line opens and closes, one space or none between tokens by hclwrite's rules, and
//! `=` signs and trailing comments aligned across consecutive lines.

use crate::lex::{Kind, Token, lex};

/// A token with the spaces written before it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tok<'s> {
    pub(crate) kind: Kind,
    pub(crate) bytes: &'s [u8],
    pub(crate) spaces: usize,
}

/// hclwrite's `Format`: the source with canonical spacing. It never fails; a file that does not
/// parse is formatted as far as its tokens allow, as Go does.
// fuzz:parser fuzz_fmt
pub fn format(src: &[u8]) -> Vec<u8> {
    let tokens = lex(src);
    render(writer_tokens(src, &tokens))
}

/// Formats a token sequence and writes it out (hclwrite's `File.Bytes`: edits are formatted too).
pub(crate) fn render(mut toks: Vec<Tok<'_>>) -> Vec<u8> {
    let lines = lines(&toks);
    indent(&mut toks, &lines);
    spaces(&mut toks, &lines);
    cells(&mut toks, &lines);
    let mut out = Vec::new();
    for t in &toks {
        out.resize(out.len() + t.spaces, b' ');
        out.extend_from_slice(t.bytes);
    }
    out
}

/// hclwrite's `writerTokens`: the spaces before a token are the bytes between it and the last one.
pub(crate) fn writer_tokens<'s>(src: &'s [u8], tokens: &[Token]) -> Vec<Tok<'s>> {
    let mut last = 0;
    tokens
        .iter()
        .map(|t| {
            let spaces = t.start.saturating_sub(last);
            last = t.end;
            Tok {
                kind: t.kind,
                bytes: t.bytes(src),
                spaces,
            }
        })
        .collect()
}

/// One line's cells, as index ranges into the tokens: everything up to an attribute's `=`
/// (`lead`), the `=` and its value (`assign`), and a trailing single-line comment (`comment`).
#[derive(Debug, Clone, Default)]
struct Line {
    lead: std::ops::Range<usize>,
    assign: Option<std::ops::Range<usize>>,
    comment: Option<std::ops::Range<usize>>,
}

fn is_newline(t: &Tok<'_>) -> bool {
    t.kind == Kind::Newline || t.kind == Kind::Comment && t.bytes.last() == Some(&b'\n')
}

fn bracket_change(k: Kind) -> i64 {
    match k {
        Kind::OBrace
        | Kind::OBrack
        | Kind::OParen
        | Kind::TemplateControl
        | Kind::TemplateInterp => 1,
        Kind::CBrace | Kind::CBrack | Kind::CParen | Kind::TemplateSeqEnd => -1,
        _ => 0,
    }
}

/// `linesForFormat`.
fn lines(toks: &[Tok<'_>]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut ended = false;
    for (i, t) in toks.iter().enumerate() {
        if t.kind == Kind::Eof {
            out.push(Line {
                lead: start..i,
                ..Line::default()
            });
            ended = true;
            break;
        }
        if is_newline(t) {
            out.push(Line {
                lead: start..i + 1,
                ..Line::default()
            });
            start = i + 1;
        }
    }
    if !ended && start < toks.len() {
        out.push(Line {
            lead: start..toks.len(),
            ..Line::default()
        });
    }
    for line in &mut out {
        split_cells(toks, line);
    }
    out
}

fn split_cells(toks: &[Tok<'_>], line: &mut Line) {
    let (s, mut e) = (line.lead.start, line.lead.end);
    if e - s > 1 && toks.get(e - 1).is_some_and(|t| t.kind == Kind::Comment) {
        line.comment = Some(e - 1..e);
        e -= 1;
        line.lead = s..e;
    }
    for i in s + 1..e {
        if toks.get(i).is_some_and(|t| t.kind == Kind::Equal) {
            let net: i64 = toks
                .get(i..e)
                .unwrap_or_default()
                .iter()
                .map(|t| bracket_change(t.kind))
                .sum();
            if net == 0 {
                line.assign = Some(i..e);
                line.lead = s..i;
            }
            break;
        }
    }
}

fn set_spaces(toks: &mut [Tok<'_>], i: usize, n: usize) {
    if let Some(t) = toks.get_mut(i) {
        t.spaces = n;
    }
}

/// `formatIndent`.
fn indent(toks: &mut [Tok<'_>], lines: &[Line]) {
    let mut stack: Vec<i64> = Vec::new();
    for line in lines {
        let Some(first) = toks
            .get(line.lead.start)
            .copied()
            .filter(|_| !line.lead.is_empty())
        else {
            continue;
        };
        if first.kind == Kind::Newline {
            set_spaces(toks, line.lead.start, 0);
            continue;
        }
        let mut net = 0;
        for t in toks.get(line.lead.clone()).unwrap_or_default() {
            net += bracket_change(t.kind);
            if t.kind == Kind::OHeredoc {
                break;
            }
        }
        if let Some(a) = &line.assign {
            net += toks
                .get(a.clone())
                .unwrap_or_default()
                .iter()
                .map(|t| bracket_change(t.kind))
                .sum::<i64>();
        }
        if net > 0 {
            set_spaces(toks, line.lead.start, 2 * stack.len());
            stack.push(net);
            continue;
        }
        let mut closed = -net;
        while closed > 0 {
            let Some(top) = stack.last_mut() else { break };
            if closed > *top {
                closed -= *top;
                stack.pop();
            } else if closed < *top {
                *top -= closed;
                closed = 0;
            } else {
                stack.pop();
                closed = 0;
            }
        }
        set_spaces(toks, line.lead.start, 2 * stack.len());
    }
}

/// `formatSpaces`: within the lead and the assign cells.
fn spaces(toks: &mut [Tok<'_>], lines: &[Line]) {
    for line in lines {
        space_cell(toks, line.lead.clone(), false);
        if let Some(a) = &line.assign {
            space_cell(toks, a.clone(), true);
        }
    }
}

fn space_cell(toks: &mut [Tok<'_>], cell: std::ops::Range<usize>, assign: bool) {
    if assign {
        set_spaces(toks, cell.start, 1);
    }
    for i in cell.clone() {
        if i + 1 >= cell.end {
            break;
        }
        let before = if i > cell.start {
            toks.get(i - 1).map(|t| t.kind)
        } else {
            None
        };
        let (Some(subject), Some(after)) = (toks.get(i).copied(), toks.get(i + 1).copied()) else {
            continue;
        };
        let n = usize::from(space_after(&subject, before, after.kind));
        set_spaces(toks, i + 1, n);
    }
}

/// `spaceAfterToken`, first half: the rules that never depend on what came before. `None` when
/// none of them decides.
fn space_fixed(s: Kind, subject: &[u8], before: Option<Kind>, after: Kind) -> Option<bool> {
    use Kind as K;
    let in_template = |k: Kind| matches!(k, K::QuotedLit | K::StringLit);
    Some(match () {
        () if after == K::Newline || after == K::Eof => false,
        () if s == K::Ident && after == K::OParen => false,
        () if s == K::Ident && after == K::DoubleColon
            || s == K::DoubleColon && after == K::Ident =>
        {
            false
        }
        () if s == K::Dot || after == K::Dot => false,
        () if after == K::Comma || after == K::Ellipsis => false,
        () if s == K::Comma => true,
        () if in_template(s)
            || matches!(s, K::OQuote | K::OHeredoc)
            || in_template(after)
            || matches!(after, K::CQuote | K::CHeredoc) =>
        {
            false
        }
        () if s == K::Ident && subject == b"in" && before == Some(K::Ident) => true,
        () if after == K::OBrack
            && (s == K::Ident || s == K::NumberLit || bracket_change(s) < 0) =>
        {
            false
        }
        () if s == K::Bang => false,
        () => return None,
    })
}

/// A minus is a negation (no space after it) unless what is before it can end an expression.
fn minus_space(before: Option<Kind>) -> bool {
    use Kind as K;
    !matches!(
        before,
        None | Some(
            K::OParen
                | K::OBrace
                | K::OBrack
                | K::Equal
                | K::Colon
                | K::Comma
                | K::Question
                | K::Plus
                | K::Star
                | K::Slash
                | K::Percent
                | K::Minus
                | K::EqualOp
                | K::NotEqual
                | K::GreaterThan
                | K::GreaterThanEq
                | K::LessThan
                | K::LessThanEq
                | K::And
                | K::Or
                | K::Bang
        )
    )
}

/// `spaceAfterToken`. `before` is `None` at the start of a cell (Go's `TokenNil`).
fn space_after(subject: &Tok<'_>, before: Option<Kind>, after: Kind) -> bool {
    use Kind as K;
    let s = subject.kind;
    if let Some(decided) = space_fixed(s, subject.bytes, before, after) {
        return decided;
    }
    match () {
        () if s == K::Minus => minus_space(before),
        () if s == K::OBrace || after == K::CBrace => !(s == K::OBrace && after == K::CBrace),
        () if matches!(s, K::TemplateInterp | K::TemplateControl) && after == K::OBrace => true,
        () if s == K::CBrace && after == K::TemplateSeqEnd => true,
        () if s == K::TemplateSeqEnd && matches!(after, K::TemplateInterp | K::TemplateControl) => {
            false
        }
        () if bracket_change(s) > 0 => false,
        () if bracket_change(after) < 0 => false,
        () => true,
    }
}

/// `Tokens.Columns`: spaces plus grapheme clusters.
fn columns(toks: &[Tok<'_>], cell: &std::ops::Range<usize>) -> usize {
    toks.get(cell.clone())
        .unwrap_or_default()
        .iter()
        .map(|t| t.spaces + clusters(t.bytes))
        .sum()
}

fn clusters(b: &[u8]) -> usize {
    crate::funcs::graphemes::count_bytes(b)
}

/// `formatCells`: align `=` across consecutive assignment lines, then trailing comments.
fn cells(toks: &mut [Tok<'_>], lines: &[Line]) {
    align(
        toks,
        lines,
        |l| l.assign.clone(),
        |t, l| columns(t, &l.lead),
    );
    align(
        toks,
        lines,
        |l| l.comment.clone(),
        |t, l| columns(t, &l.lead) + l.assign.as_ref().map_or(0, |a| columns(t, a)),
    );
}

fn align(
    toks: &mut [Tok<'_>],
    lines: &[Line],
    cell: impl Fn(&Line) -> Option<std::ops::Range<usize>>,
    width: impl Fn(&[Tok<'_>], &Line) -> usize,
) {
    let mut chain: Vec<&Line> = Vec::new();
    let mut max = 0;
    let close = |toks: &mut [Tok<'_>], chain: &mut Vec<&Line>, max: &mut usize| {
        for l in chain.drain(..) {
            let w = width(toks, l);
            if let Some(c) = cell(l) {
                set_spaces(toks, c.start, *max - w + 1);
            }
        }
        *max = 0;
    };
    for line in lines {
        if cell(line).is_none() {
            close(toks, &mut chain, &mut max);
            continue;
        }
        max = max.max(width(toks, line));
        chain.push(line);
    }
    close(toks, &mut chain, &mut max);
}
