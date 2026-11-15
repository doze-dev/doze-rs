//! What a stack's doze.hcl says, read without loading it (Go `view/refs.go`, and the part of `config.LoadShape` a
//! stopped stack's page needs): its blocks, a few attributes of each (`run`, `port`, `domains`, `enabled`,
//! `version`), what each refers to, in order, and the lines that do the referring, for the Grid's tooltips: the file's
//! own words, not a paraphrase.
//!
//! This is not an HCL parser. It finds blocks and attributes by their shape and references by HCL's own rule (a
//! traversal rooted at `app`, `module`, `service` or `task`, in code or inside an interpolation, never in a plain
//! string), which is all the Grid and the stopped page read. Whether the file loads is the CLI's to say
//! (`doze --json status`, [`super::parse_status`]).

mod lex;

use std::collections::BTreeMap;
use std::path::Path;

use lex::{Tok, Token, lex};

/// The references doze.hcl writes, by the block that writes them, then the block referred to (or `after`):
/// `refs["orders"]["catalog"]` is `CATALOG_URL = app.catalog.url`.
pub type Refs = BTreeMap<String, BTreeMap<String, String>>;

/// The roots of a reference to another block.
const REF_KINDS: [&str; 4] = ["app", "module", "service", "task"];

/// How deep blocks may nest before the rest is skipped.
const MAX_DEPTH: usize = 32;

/// A file, as far as it was read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HclFile {
    /// `stack = "…"`.
    pub stack: String,
    /// Its `app`, `module`, `service` and `task` blocks, in the file's order.
    pub blocks: Vec<Block>,
    pub refs: Refs,
}

/// One block that declares an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// `app`, `module`, `service` or `task`.
    pub kind: String,
    /// Its name: the last label.
    pub name: String,
    /// A module's type (`module postgres "db"`: `postgres`).
    pub ty: String,
    pub run: String,
    pub port: Option<u32>,
    pub domains: Vec<String>,
    /// `enabled = false` turns it off in the file.
    pub enabled: bool,
    pub version: String,
    /// The blocks it refers to, in the order it does, each once.
    pub needs: Vec<String>,
    /// The line it starts on.
    pub line: usize,
}

/// Reads `src` (one file). A file that does not parse gives what could be read of it.
// fuzz:parser fuzz_hcl_scan
pub fn scan(src: &str) -> HclFile {
    let toks = lex(src);
    let lines: Vec<&str> = src.split('\n').collect();
    let mut p = Parser {
        src,
        lines,
        toks: &toks,
        i: 0,
        out: HclFile::default(),
    };
    p.file();
    p.out
}

/// The references of a stack's files: doze.hcl, then each `*.doze.hcl` beside it, by name. `None` when there is none
/// to read (Go `RefsIn`).
pub fn refs_in(dir: &Path) -> Option<Refs> {
    let mut files = vec![dir.join("doze.hcl")];
    let mut siblings: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".doze.hcl"))
        })
        .collect();
    siblings.sort();
    files.extend(siblings);
    let mut out: Option<Refs> = None;
    for f in files {
        let Ok(src) = std::fs::read_to_string(&f) else {
            continue;
        };
        let refs = out.get_or_insert_with(Refs::new);
        for (from, m) in scan(&src).refs {
            let have = refs.entry(from).or_default();
            for (to, line) in m {
                have.entry(to).or_insert(line);
            }
        }
    }
    out
}

struct Parser<'s, 't> {
    src: &'s str,
    lines: Vec<&'s str>,
    toks: &'t [Token<'s>],
    i: usize,
    out: HclFile,
}

impl<'s> Parser<'s, '_> {
    fn tok(&self, at: usize) -> Option<&Tok<'s>> {
        self.toks.get(at).map(|t| &t.tok)
    }

    fn file(&mut self) {
        while let Some(t) = self.toks.get(self.i) {
            let Tok::Word(w) = t.tok else {
                self.i += 1;
                continue;
            };
            if self.tok(self.i + 1) == Some(&Tok::Punct('=')) {
                let (start, end) = self.expr(self.i + 2);
                if w == "stack"
                    && let Some(s) = single_str(self.toks.get(start..end).unwrap_or(&[]))
                {
                    self.out.stack = s;
                }
                self.i = end;
            } else if let Some((labels, open)) = self.labels(self.i + 1) {
                self.block(w, &labels, t.line, open);
            } else {
                self.i += 1;
            }
        }
    }

    /// The labels after a block's type, and where its `{` is.
    fn labels(&self, mut at: usize) -> Option<(Vec<String>, usize)> {
        let mut labels = Vec::new();
        loop {
            match self.tok(at)? {
                Tok::Word(w) => labels.push((*w).to_owned()),
                Tok::Str(s) => labels.push(s.clone()),
                Tok::Punct('{') => return Some((labels, at)),
                _ => return None,
            }
            at += 1;
        }
    }

    /// A top-level block whose `{` is at `open`.
    fn block(&mut self, kind: &str, labels: &[String], line: usize, open: usize) {
        self.i = open + 1;
        let Some(name) = labels.last().filter(|_| REF_KINDS.contains(&kind)) else {
            self.skip_body(0);
            return;
        };
        let mut b = Block {
            kind: kind.to_owned(),
            name: name.clone(),
            ty: if kind == "module" && labels.len() > 1 {
                labels.first().cloned().unwrap_or_default()
            } else {
                String::new()
            },
            run: String::new(),
            port: None,
            domains: Vec::new(),
            enabled: true,
            version: String::new(),
            needs: Vec::new(),
            line,
        };
        self.body(&mut b, 0);
        self.out.blocks.push(b);
    }

    /// A block's body, up to and past its `}`; nested blocks' attributes count as the block's (Go `walkBody`).
    fn body(&mut self, b: &mut Block, depth: usize) {
        while let Some(t) = self.toks.get(self.i) {
            match t.tok {
                Tok::Punct('}') => {
                    self.i += 1;
                    return;
                }
                Tok::Word(w) if self.tok(self.i + 1) == Some(&Tok::Punct('=')) => {
                    let (start, end) = self.expr(self.i + 2);
                    self.attr(b, w, t.start, start, end);
                    self.i = end;
                }
                Tok::Word(_) => match self.labels(self.i + 1) {
                    Some((_, open)) if depth < MAX_DEPTH => {
                        self.i = open + 1;
                        self.body(b, depth + 1);
                    }
                    Some((_, open)) => {
                        self.i = open + 1;
                        self.skip_body(depth);
                    }
                    None => self.i += 1,
                },
                _ => self.i += 1,
            }
        }
    }

    /// Past a body this reader does not look into.
    fn skip_body(&mut self, _depth: usize) {
        let mut depth = 0usize;
        while let Some(t) = self.toks.get(self.i) {
            self.i += 1;
            match t.tok {
                Tok::Punct('{') => depth += 1,
                Tok::Punct('}') if depth == 0 => return,
                Tok::Punct('}') => depth -= 1,
                _ => {}
            }
        }
    }

    /// An expression starting at `from`: to the line's end, or the block's `}`, outside any brackets. Returns its
    /// tokens' range.
    fn expr(&self, from: usize) -> (usize, usize) {
        let mut depth = 0usize;
        let mut at = from;
        while let Some(t) = self.tok(at) {
            match t {
                Tok::Newline if depth == 0 => break,
                Tok::Punct('}') if depth == 0 => break,
                Tok::Punct('[' | '(' | '{') => depth += 1,
                Tok::Punct(']' | ')' | '}') => depth = depth.saturating_sub(1),
                _ => {}
            }
            at += 1;
        }
        (from, at)
    }

    /// One attribute of `b`: `name` at byte `at`, its expression the tokens `start..end`.
    fn attr(&mut self, b: &mut Block, name: &str, at: usize, start: usize, end: usize) {
        let toks = self.toks.get(start..end).unwrap_or(&[]);
        match name {
            "run" => b.run = single_str(toks).unwrap_or_default(),
            "port" => b.port = single_word(toks).and_then(|w| w.parse().ok()),
            "enabled" => b.enabled = single_word(toks) != Some("false"),
            "version" => {
                b.version = single_word(toks)
                    .map(str::to_owned)
                    .or_else(|| single_str(toks))
                    .unwrap_or_default()
            }
            "domains" => {
                b.domains = toks
                    .iter()
                    .filter_map(|t| match &t.tok {
                        Tok::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
            }
            _ => {}
        }
        let refs = self.out.refs.entry(b.name.clone()).or_default();
        let whole_end = toks.last().map_or(at, |t| t.end);
        let after = (name == "after").then(|| one_line(self.src.get(at..whole_end).unwrap_or("")));
        if let Some(a) = &after {
            refs.entry("after".to_owned()).or_insert_with(|| a.clone());
        }
        for (root, target, line) in traversals(toks) {
            if !b.needs.iter().any(|n| n == target) {
                b.needs.push(target.to_owned());
            }
            if refs.contains_key(target) {
                continue;
            }
            // In `after`, the whole attribute (the first `after` the block has); elsewhere, the reference's line.
            let text = match &after {
                Some(_) => refs.get("after").cloned().unwrap_or_default(),
                None => one_line(
                    self.lines
                        .get(line.saturating_sub(1))
                        .copied()
                        .unwrap_or(""),
                ),
            };
            refs.insert(
                target.to_owned(),
                shorten(&text, &format!("{root}.{target}")),
            );
        }
    }
}

/// The references in an expression: `root.target`, rooted at a block kind, with the line each starts on.
fn traversals<'s>(toks: &[Token<'s>]) -> Vec<(&'s str, &'s str, usize)> {
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let Tok::Word(root) = t.tok else { continue };
        if !REF_KINDS.contains(&root) {
            continue;
        }
        let after_dot = i > 0
            && toks
                .get(i - 1)
                .is_some_and(|p| p.tok == Tok::Punct('.') && p.end == t.start);
        let (Some(dot), Some(name)) = (toks.get(i + 1), toks.get(i + 2)) else {
            continue;
        };
        if let (false, Tok::Punct('.'), Tok::Word(target)) = (after_dot, &dot.tok, &name.tok)
            && dot.start == t.end
            && name.start == dot.end
        {
            out.push((root, *target, t.line));
        }
    }
    out
}

fn single_str(toks: &[Token<'_>]) -> Option<String> {
    match toks {
        [
            Token {
                tok: Tok::Str(s), ..
            },
        ] => Some(s.clone()),
        _ => None,
    }
}

fn single_word<'s>(toks: &[Token<'s>]) -> Option<&'s str> {
    match toks {
        [
            Token {
                tok: Tok::Word(w), ..
            },
        ] => Some(w),
        _ => None,
    }
}

/// A line of the file as a tooltip shows it: no comment, no trailing comma, the spaces that align it closed up.
pub(crate) fn one_line(s: &str) -> String {
    let s = match s.find(" #") {
        Some(i) if !s.get(..i).is_some_and(|h| h.contains('"')) => s.get(..i).unwrap_or(s),
        _ => s,
    };
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.strip_suffix(',').unwrap_or(&s);
    s.replace("[ ", "[").replace(" ]", "]")
}

/// A long line kept to its key and the reference in it: `DATABASE_URL = "…${module.postgres.address}…"`.
pub(crate) fn shorten(s: &str, reference: &str) -> String {
    const MOST: usize = 64;
    if s.chars().count() <= MOST {
        return s.to_owned();
    }
    let (key, expr) = s.split_once(" = ").unwrap_or(("", s));
    let Some(i) = expr.find(reference) else {
        let mut cut: String = s.chars().take(MOST - 1).collect();
        cut.push('…');
        return cut;
    };
    let tail = expr.get(i + reference.len()..).unwrap_or("");
    let more = tail
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        .count();
    let part = expr.get(i..i + reference.len() + more).unwrap_or(reference);
    let before = expr.get(..i).unwrap_or("");
    let part = if before.starts_with('"') || before.contains("${") {
        format!("\"…${{{part}}}…\"")
    } else {
        format!("…{part}…")
    };
    if key.is_empty() {
        part
    } else {
        format!("{key} = {part}")
    }
}

#[cfg(test)]
mod tests;
