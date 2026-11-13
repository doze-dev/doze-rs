//! `cargo xtask themes [--check]`: the desktop app's themes, converted from the Go app's design tokens.
//!
//! `apps/doze-app/themes/tokens.css` is a copy of the Go app's `internal/desk/frontend/src/tokens.css` (written there by
//! `go generate ./internal/brand`): five palettes, each dark and light, as `:root[data-theme][data-mode]` blocks of
//! `--name: #hex;`. This writes `apps/doze-app/themes/doze.json`, one gpui-component theme set with the ten themes: the
//! component colors mapped from the tokens (docs/port/app/PLAN.md §4.6), and doze's own tokens kept whole under `doze`
//! (gpui-component ignores keys it does not know), so the app paints with the same names the React app did.
//! `--check` fails when the JSON is not what the CSS gives (CI runs it, as it runs `codegen --check`).

use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

const CSS: &str = "apps/doze-app/themes/tokens.css";
const JSON: &str = "apps/doze-app/themes/doze.json";

/// doze's tokens, in the order the CSS writes them; each theme must have them all.
const TOKENS: [&str; 20] = [
    "bg",
    "side",
    "panel",
    "line",
    "line2",
    "well",
    "text",
    "dim",
    "bright",
    "accent",
    "good",
    "warn",
    "cool",
    "bad",
    "extra",
    "sel",
    "pick",
    "track",
    "ink",
    "on-accent",
];

/// gpui-component's color keys, each from a doze token (an `@` suffix is an alpha byte).
const COMPONENT: [(&str, &str); 40] = [
    ("background", "bg"),
    ("foreground", "text"),
    ("border", "line"),
    ("input.border", "line2"),
    ("caret", "accent"),
    ("ring", "accent"),
    ("selection.background", "accent@59"),
    ("muted.background", "panel"),
    ("muted.foreground", "dim"),
    ("accent.background", "sel"),
    ("accent.foreground", "bright"),
    ("primary.background", "accent"),
    ("primary.hover.background", "accent"),
    ("primary.active.background", "accent"),
    ("primary.foreground", "on-accent"),
    ("secondary.background", "panel"),
    ("secondary.hover.background", "sel"),
    ("secondary.active.background", "pick"),
    ("secondary.foreground", "text"),
    ("popover.background", "panel"),
    ("popover.foreground", "text"),
    ("list.background", "panel"),
    ("list.hover.background", "sel"),
    ("list.active.background", "pick"),
    ("list.active.border", "accent"),
    ("danger.background", "bad"),
    ("success.background", "good"),
    ("warning.background", "warn"),
    ("info.background", "cool"),
    ("sidebar.background", "side"),
    ("sidebar.border", "line"),
    ("title_bar.background", "side"),
    ("title_bar.border", "line"),
    ("status_bar.background", "bg"),
    ("scrollbar.thumb.background", "line2"),
    ("switch.background", "track"),
    ("skeleton.background", "sel"),
    ("chart.1", "accent"),
    ("chart.2", "cool"),
    ("chart.3", "good"),
];

/// One `:root[data-theme][data-mode]` block.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    palette: String,
    mode: String,
    tokens: Vec<(String, String)>,
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let css = match fs::read_to_string(CSS) {
        Ok(s) => s,
        Err(e) => return fail(&format!("reading {CSS}: {e}")),
    };
    let json = match convert(&css) {
        Ok(j) => j,
        Err(e) => return fail(&format!("{CSS}: {e}")),
    };
    if check {
        return match fs::read_to_string(JSON) {
            Ok(old) if old == json => {
                println!("themes: {JSON} is up to date");
                ExitCode::SUCCESS
            }
            _ => fail(&format!("{JSON} is stale: run `cargo xtask themes`")),
        };
    }
    match fs::write(JSON, json) {
        Ok(()) => {
            println!("themes: wrote {JSON}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("writing {JSON}: {e}")),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("xtask themes: {message}");
    ExitCode::FAILURE
}

/// The theme set for the CSS.
fn convert(css: &str) -> Result<String, String> {
    let blocks = blocks(css)?;
    if blocks.len() != 10 {
        return Err(format!(
            "expected 10 palette blocks (5 palettes x dark/light), found {}",
            blocks.len()
        ));
    }
    let mut out = String::from(
        "{\n  \"name\": \"doze\",\n  \"author\": \"doze (converted from tokens.css by `cargo xtask themes`; do not edit)\",\n  \"themes\": [\n",
    );
    for (i, b) in blocks.iter().enumerate() {
        theme(&mut out, b)?;
        out.push_str(if i + 1 < blocks.len() { ",\n" } else { "\n" });
    }
    out.push_str("  ]\n}\n");
    Ok(out)
}

fn theme(out: &mut String, b: &Block) -> Result<(), String> {
    let get = |name: &str| -> Result<&str, String> {
        b.tokens
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| format!("{} {}: no --{name}", b.palette, b.mode))
    };
    let _ = writeln!(out, "    {{");
    let _ = writeln!(out, "      \"name\": \"doze {} {}\",", b.palette, b.mode);
    let _ = writeln!(out, "      \"mode\": \"{}\",", b.mode);
    out.push_str("      \"font.family\": \"Inter\",\n      \"font.size\": 13,\n");
    out.push_str(
        "      \"mono_font.family\": \"JetBrains Mono\",\n      \"mono_font.size\": 12,\n",
    );
    out.push_str("      \"radius\": 7,\n      \"radius.lg\": 10,\n      \"shadow\": false,\n");
    out.push_str("      \"colors\": {\n");
    for (i, (key, from)) in COMPONENT.iter().enumerate() {
        let (token, alpha) = from.split_once('@').unwrap_or((from, ""));
        let comma = if i + 1 < COMPONENT.len() { "," } else { "" };
        let _ = writeln!(
            out,
            "        \"{key}\": \"{}{alpha}\"{comma}",
            get(token)?.to_lowercase()
        );
    }
    out.push_str("      },\n      \"doze\": {\n");
    let _ = writeln!(out, "        \"palette\": \"{}\",", b.palette);
    for (i, t) in TOKENS.iter().enumerate() {
        let comma = if i + 1 < TOKENS.len() { "," } else { "" };
        let _ = writeln!(
            out,
            "        \"{}\": \"{}\"{comma}",
            t.replace('-', "_"),
            get(t)?.to_lowercase()
        );
    }
    out.push_str("      }\n    }");
    Ok(())
}

/// Every `:root[data-theme="…"][data-mode="…"] { … }` block, in order.
fn blocks(css: &str) -> Result<Vec<Block>, String> {
    const HEAD: &str = ":root[data-theme=\"";
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(at) = rest.find(HEAD) {
        rest = rest.get(at + HEAD.len()..).unwrap_or("");
        let (palette, after) = rest
            .split_once("\"][data-mode=\"")
            .ok_or("a data-theme without a data-mode")?;
        let (mode, after) = after.split_once("\"]").ok_or("an unclosed data-mode")?;
        let (_, body) = after.split_once('{').ok_or("a block without a body")?;
        let (body, after) = body.split_once('}').ok_or("an unclosed block")?;
        let tokens = body
            .split(';')
            .filter_map(|d| {
                let (k, v) = d.trim().split_once(':')?;
                Some((k.trim().strip_prefix("--")?.to_owned(), v.trim().to_owned()))
            })
            .collect();
        out.push(Block {
            palette: palette.to_owned(),
            mode: mode.to_owned(),
            tokens,
        });
        rest = after;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{blocks, convert};

    #[test]
    fn a_block_is_read_and_converted() {
        let css = ":root { --bg: #000; }\n:root[data-theme=\"violet\"][data-mode=\"dark\"] {\n  --bg: #15131D;\n  --on-accent: #fff;\n}\n";
        let b = blocks(css).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].palette, "violet");
        assert_eq!(b[0].mode, "dark");
        assert_eq!(b[0].tokens[0], ("bg".to_owned(), "#15131D".to_owned()));
        assert!(convert(css).unwrap_err().contains("expected 10"));
    }

    #[test]
    fn the_real_tokens_convert_to_ten_themes() {
        let css = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../apps/doze-app/themes/tokens.css"
        ))
        .unwrap();
        let json = convert(&css).unwrap();
        assert_eq!(json.matches("\"mode\":").count(), 10);
        assert!(json.contains("\"name\": \"doze violet dark\""));
        assert!(json.contains("\"selection.background\": \"#bd93f959\""));
    }
}
