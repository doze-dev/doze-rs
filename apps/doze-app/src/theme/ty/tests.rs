//! The app names its type only through `theme::ty`: a raw size, line height or weight anywhere else fails here.

use std::path::{Path, PathBuf};

/// Calls whose argument at these positions is a size or a weight (`None`: every argument). The methods are GPUI's;
/// the functions are widgets' and kit's.
const TYPED: &[(&str, &[usize])] = &[
    (".text_size(", &[0]),
    (".line_height(", &[0]),
    (".font_weight(", &[0]),
    ("tracked(", &[1, 2]),
    ("kbd_plain(", &[1, 3]),
    ("kbd_chip(", &[2, 3]),
    ("mono(", &[1]),
    ("heading(", &[2]),
];

/// GPUI's own presets and raw weights, which bypass the table.
const BANNED: &[&str] = &[
    "FontWeight(",
    "FontWeight::",
    ".text_xs()",
    ".text_sm()",
    ".text_base()",
    ".text_lg()",
    ".text_xl()",
    ".text_2xl()",
    ".text_3xl()",
    ".font_bold()",
    ".font_semibold()",
    ".font_medium()",
    ".font_light()",
    ".font_normal()",
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The text inside the parens opening at byte `open` of `s`.
fn inside(s: &str, open: usize) -> &str {
    let mut depth = 0;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &s[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    ""
}

/// Top-level comma-separated arguments.
fn args(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// A number written out: a digit not inside a name or a field (`LH_18` and `z.0` are not numbers, `18.` is).
fn has_literal(s: &str) -> bool {
    let mut prev = ' ';
    for c in s.chars() {
        if c.is_ascii_digit() && !(prev.is_alphanumeric() || prev == '_' || prev == '.') {
            return true;
        }
        prev = c;
    }
    false
}

/// Every raw size or weight in one file's source, as `call: argument`.
fn raw_in(src: &str) -> Vec<String> {
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut found = Vec::new();
    for (call, at) in TYPED {
        for (i, _) in code.match_indices(call) {
            let before = code[..i].chars().next_back().unwrap_or(' ');
            if !call.starts_with('.')
                && (before.is_alphanumeric() || before == '_' || before == '.')
            {
                continue;
            }
            let a = args(inside(&code, i + call.len() - 1));
            for n in at.iter() {
                if let Some(arg) = a.get(*n).filter(|x| has_literal(x)) {
                    found.push(format!("{call}…{})", arg.trim()));
                }
            }
        }
    }
    found.extend(
        BANNED
            .iter()
            .filter(|b| code.contains(*b))
            .map(|b| (*b).to_owned()),
    );
    found
}

#[test]
fn no_raw_type() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut all = Vec::new();
    files(&src, &mut all);
    let mut bad = Vec::new();
    for f in all.iter().filter(|f| !f.starts_with(src.join("theme/ty"))) {
        if f.ends_with("theme/ty.rs") {
            continue;
        }
        for r in raw_in(&std::fs::read_to_string(f).unwrap()) {
            bad.push(format!("{}: {r}", f.strip_prefix(&src).unwrap().display()));
        }
    }
    assert!(
        bad.is_empty(),
        "raw type outside theme::ty (use its tokens):\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_check_catches_raw_type() {
    assert_eq!(raw_in("div().text_size(px(12.))").len(), 1);
    assert_eq!(raw_in("div().line_height(z.p(18.))").len(), 1);
    assert_eq!(raw_in("kbd_chip(t, k, 11., ty::SEMIBOLD, 6.)").len(), 1);
    assert_eq!(raw_in("x.font_weight(FontWeight(600.))").len(), 2);
    assert!(raw_in("div().text_size(px(ty::SM)).line_height(px(ty::LH_18))").is_empty());
    assert!(raw_in("kbd_chip(t, k, ty::XS, ty::SEMIBOLD, 6.)").is_empty());
    assert!(raw_in("// .text_size(px(12.))").is_empty());
}
