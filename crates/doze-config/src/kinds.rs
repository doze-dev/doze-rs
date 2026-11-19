//! The words of a stack: which top-level blocks there are, the old forms refused with what to write
//! now, a module block's header, and `doze = ">= x"` (Go's `kinds.go`).

use crate::body::Block;
use crate::error::{At, Error, Result, levenshtein, pos_err};
use crate::files::Files;
use crate::model::{COMMAND_ENGINE, Kind, title};
use crate::version;
use doze_hcl::Value;
use doze_hcl::diag::Range;
use doze_hcl::eval::{Context, Evaluator};
use doze_hcl::syntax::Source;
use doze_hcl::value::quote::go_quote;
use hcl_edit::Span;

/// Top-level blocks that are not a module, an app, a service or a task.
pub const RESERVED: [&str; 5] = ["defaults", "tls", "variable", "locals", "output"];
/// Top-level blocks a later release reads.
pub const FUTURE: [&str; 1] = ["recall"];

pub fn is_kind(s: &str) -> bool {
    Kind::parse(s).is_some()
}

fn reserved(s: &str) -> bool {
    RESERVED.contains(&s)
}

/// A module or app block's kind, engine type and label.
#[derive(Debug, Clone)]
pub struct Head {
    pub kind: Kind,
    pub ty: String,
    pub label: String,
}

impl Head {
    pub fn title(&self, name: &str) -> String {
        title(self.kind, &self.ty, name)
    }
}

/// Go's `headOf`.
pub fn head_of(files: &Files, block: &Block<'_>) -> Result<Head> {
    if block.ty == "app" {
        return Ok(Head {
            kind: Kind::App,
            ty: COMMAND_ENGINE.into(),
            label: block.label(0).into(),
        });
    }
    let (ty, name) = (block.label(0), block.label(1));
    if ty == COMMAND_ENGINE {
        return Err(pos_err(
            files,
            &block.def,
            "`process` is not a module",
            format!(
                "an app is declared as: app {} {{ run = \"…\" }}",
                go_quote(name)
            ),
        ));
    }
    if reserved(ty) || FUTURE.contains(&ty) || is_kind(ty) {
        return Err(pos_err(
            files,
            &block.def,
            format!("{} is a keyword, not a module type", go_quote(ty)),
            "a module block reads: module <type> \"<name>\" { … }, for example: module postgres \"db\" { … }",
        ));
    }
    Ok(Head {
        kind: Kind::Module,
        ty: ty.into(),
        label: name.into(),
    })
}

fn range(src: &Source, span: Option<std::ops::Range<usize>>) -> Range {
    span.map(|s| src.range(s)).unwrap_or_default()
}

fn text(src: &Source, span: Option<std::ops::Range<usize>>) -> String {
    span.and_then(|s| src.text().get(s))
        .unwrap_or("\"…\"")
        .to_owned()
}

/// Go's `checkOldForms`: the forms doze used to read, refused with what to write now.
pub fn check_old_forms(files: &Files) -> Result<()> {
    for f in &files.files {
        let at = |r: Range| At::new(&f.name, r);
        if let Some(a) = f.body.get_attribute("name") {
            return Err(pos_err(
                files,
                &at(range(&f.src, a.key.span())),
                "`name` is now `stack`",
                format!("write: stack = {}", text(&f.src, a.value.span())),
            ));
        }
        if let Some(a) = f.body.get_attribute("listen") {
            return Err(pos_err(
                files,
                &at(range(&f.src, a.key.span())),
                "a stack-wide `listen` is gone: doze never gave instances ports from it",
                "delete it; a module listens on its own port (set `port` on it to change it), and `listen` on one block still takes a unix:/path",
            ));
        }
        for b in f.body.blocks() {
            let ty = b.ident.as_str();
            if reserved(ty) || is_kind(ty) {
                continue;
            }
            let labels: Vec<String> = b.labels.iter().map(|l| l.as_str().to_owned()).collect();
            let last = b.labels.last().and_then(|l| l.span()).or(b.ident.span());
            let def = at(Range::between(
                range(&f.src, b.ident.span()),
                range(&f.src, last),
            ));
            return Err(old_block(files, &def, ty, &labels));
        }
    }
    Ok(())
}

fn old_block(files: &Files, def: &At, ty: &str, labels: &[String]) -> Error {
    let label = labels.last().map_or("<name>", String::as_str);
    let q = go_quote(label);
    let (summary, detail) = if FUTURE.contains(&ty) {
        (
            format!("the `{ty}` block isn't in this release of doze"),
            "remove it for now; a later release reads it".to_owned(),
        )
    } else if ty == "modules" && labels.is_empty() {
        ("the `modules` block is gone".to_owned(), "doze has one registry and picks each module's release itself; doze.lock pins it, and `doze modules upgrade` moves the pin".to_owned())
    } else if ty == "use" && labels.len() == 2 {
        (
            "`use` is now `module`".to_owned(),
            format!(
                "write: module {} {q} {{ … }}",
                labels.first().map_or("", String::as_str)
            ),
        )
    } else if ty == COMMAND_ENGINE {
        (
            "`process` is now `app`".to_owned(),
            format!(
                "write: app {q} {{ … }}, with `run` in place of `command` and `dir` in place of `cwd`"
            ),
        )
    } else if let Some(kw) = near_keyword(ty) {
        (
            format!("unknown block {}", go_quote(ty)),
            format!("did you mean {}?", go_quote(kw)),
        )
    } else if labels.len() == 1 {
        (
            "a module block starts with `module`".to_owned(),
            format!("write: module {ty} {q} {{ … }}"),
        )
    } else {
        (
            format!("unknown block {}", go_quote(ty)),
            "a stack declares module, app, service and task blocks".to_owned(),
        )
    };
    pos_err(files, def, summary, detail)
}

/// A keyword one edit away (Go ranges over a map here; the candidates are far enough apart that
/// at most one is ever one edit away).
fn near_keyword(s: &str) -> Option<&'static str> {
    Kind::ALL
        .iter()
        .map(|k| k.as_str())
        .chain(RESERVED)
        .chain(FUTURE)
        .find(|c| levenshtein(s, c) == 1)
}

/// Go's `checkDozeVersion`: `doze = ">= x"` is text, a version, and not newer than this doze.
pub fn check_doze_version(files: &Files, current: &str) -> Result<()> {
    for f in &files.files {
        let Some(a) = f.body.get_attribute("doze") else {
            continue;
        };
        let at = At::new(&f.name, range(&f.src, a.span()));
        let (v, ds) = Evaluator::new(&f.src).eval(&a.value, &Context::default());
        let Value::String(raw) = v else {
            return Err(pos_err(
                files,
                &at,
                "`doze` is the oldest doze this stack needs, as text",
                "write: doze = \">= 0.2\"",
            ));
        };
        if ds.has_errors() {
            return Err(pos_err(
                files,
                &at,
                "`doze` is the oldest doze this stack needs, as text",
                "write: doze = \">= 0.2\"",
            ));
        }
        let raw = raw.trim();
        let want = raw.strip_prefix(">=").unwrap_or(raw).trim();
        if !version::released(want) {
            return Err(pos_err(
                files,
                &at,
                format!("`doze = {}` is not a doze version", go_quote(raw)),
                "write: doze = \">= 0.2\", the oldest doze this stack needs",
            ));
        }
        if version::needs_newer(current, want) {
            return Err(pos_err(
                files,
                &at,
                format!(
                    "this stack needs doze {want} or newer; this doze is {}",
                    current.trim_start_matches('v')
                ),
                "update doze (brew upgrade doze, or in the app: Settings › About), then run the command again",
            ));
        }
    }
    Ok(())
}
