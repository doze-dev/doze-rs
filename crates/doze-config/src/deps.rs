//! References between blocks: what a traversal names, `after = [...]`, and the dependencies an
//! instance has (Go's `declared.resolve`, `parseAfter`, `instanceDeps` and their errors).

use crate::error::{At, Error, Result, Sources, levenshtein, nearest, pos_err};
use crate::kinds::is_kind;
use crate::load::Declared;
use crate::model::{COMMAND_ENGINE, Condition, Config, Kind, Ref};
use crate::refs::Traversal;
use doze_hcl::decode::Attr;
use doze_hcl::value::quote::go_quote;
use hcl_edit::Span;
use hcl_edit::expr::{Expression as E, TraversalOperator as Op};
use std::collections::BTreeMap;

/// Go's `declared.resolve`: the block a traversal names, if it names one; an error if it names
/// one wrongly.
pub(crate) fn resolve(
    src: &dyn Sources,
    known: &Declared,
    file: &str,
    t: &Traversal,
) -> Result<Option<Ref>> {
    let at = At::new(file, t.range);
    let Some(root) = Kind::parse(&t.root) else {
        if let Some(name) = t.name()
            && (known.module_types.contains(&t.root) || t.root == COMMAND_ENGINE)
        {
            let kind = known
                .kind
                .get(name)
                .copied()
                .unwrap_or(if t.root == COMMAND_ENGINE {
                    Kind::App
                } else {
                    Kind::Module
                });
            return Err(pos_err(
                src,
                &at,
                "a reference starts with the kind, not the type",
                format!(
                    "write {}.{name} in place of {}.{name}",
                    kind.as_str(),
                    t.root
                ),
            ));
        }
        return Ok(None);
    };
    let Some(name) = t.name() else {
        return Err(pos_err(
            src,
            &at,
            "incomplete reference",
            format!("a reference reads {}.<name>.<attribute>", t.root),
        ));
    };
    match known.kind.get(name) {
        None => {
            let mut detail = format!("no {} named {} is declared", t.root, go_quote(name));
            if let Some(s) = nearest(name, &known.names(root)) {
                detail.push_str(&format!(". Did you mean {}.{s}?", t.root));
            }
            Err(pos_err(
                src,
                &at,
                format!("reference to undeclared {} {}", t.root, go_quote(name)),
                detail,
            ))
        }
        Some(k) if *k != root => Err(pos_err(
            src,
            &at,
            format!(
                "{} is {}, not {}",
                go_quote(name),
                k.article(),
                root.article()
            ),
            format!("write {}.{name}", k.as_str()),
        )),
        Some(_) => Ok(Some(Ref {
            kind: root,
            name: name.to_owned(),
        })),
    }
}

pub(crate) fn self_reference(src: &dyn Sources, title: &str, file: &str, t: &Traversal) -> Error {
    pos_err(
        src,
        &At::new(file, t.range),
        format!("{title} refers to itself"),
        "a block cannot read its own attributes; write the value itself (an app's own port is in $PORT)",
    )
}

/// Go's `portlessURL`: `app.x.url` of an app (or service) with no port has no url.
pub(crate) fn portless_url(
    src: &dyn Sources,
    cfg: &Config,
    file: &str,
    r: &Ref,
    t: &Traversal,
) -> Result<()> {
    if t.steps.len() < 2 || !matches!(r.kind, Kind::App | Kind::Service) || t.attr() != Some("url")
    {
        return Ok(());
    }
    let port = match r.kind {
        Kind::Service => cfg.service(&r.name).map_or(0, |s| s.port),
        _ => cfg.instance(&r.name).map_or(0, |i| i.port),
    };
    if port != 0 {
        return Ok(());
    }
    let (k, n) = (r.kind.as_str(), go_quote(&r.name));
    Err(pos_err(
        src,
        &At::new(file, t.range),
        format!("{k} {n} has no port, so it has no url"),
        format!("give {k} {n} a port = … to reach it by url, or read another of its attributes"),
    ))
}

const AFTER_WANT: &str = "after takes a list of references to blocks, for example: after = [task.migrate, module.db, started(app.cloud)]";

/// Go's `parseAfter`: the references in `after`, and how long each one is waited for.
pub(crate) fn parse_after(
    src: &dyn Sources,
    attr: &Attr<'_>,
    known: &Declared,
) -> Result<(Vec<Ref>, BTreeMap<String, Condition>)> {
    let range_of = |e: &E| {
        At::new(
            attr.file,
            e.span().map(|s| attr.src.range(s)).unwrap_or_default(),
        )
    };
    let E::Array(items) = attr.expr else {
        return Err(pos_err(
            src,
            &At::new(attr.file, attr.range),
            "invalid after",
            AFTER_WANT,
        ));
    };
    let mut out = Vec::new();
    let mut conds = BTreeMap::new();
    for item in items.iter() {
        let (cond, e) = match item {
            E::FuncCall(call) => {
                let name = call.name.name.as_str();
                let cond = match name {
                    "started" => Condition::Started,
                    "lazy" => Condition::Lazy,
                    "ready" | "healthy" => Condition::Healthy,
                    _ => {
                        return Err(pos_err(
                            src,
                            &range_of(item),
                            format!("after: {name}(…) is not a way to wait"),
                            "write a bare reference to wait until it is ready, started(module.x) to wait until it runs, or lazy(module.x) to order without starting it",
                        ));
                    }
                };
                let args: Vec<&E> = call.args.iter().collect();
                let [one] = args.as_slice() else {
                    return Err(pos_err(
                        src,
                        &range_of(item),
                        format!("after: {name} takes one reference"),
                        AFTER_WANT,
                    ));
                };
                (cond, *one)
            }
            e => (Condition::Healthy, e),
        };
        let t = abs_traversal(e, attr.src)
            .filter(|t| t.steps.len() == 1 && is_kind(&t.root))
            .ok_or_else(|| pos_err(src, &range_of(e), "invalid after", AFTER_WANT))?;
        if let Some(r) = resolve(src, known, attr.file, &t)? {
            if cond != Condition::Healthy {
                conds.insert(r.name.clone(), cond);
            }
            out.push(r);
        }
    }
    Ok((out, conds))
}

/// hcl's `AbsTraversalForExpr`: a bare `a.b.c` (attribute and literal index steps only).
fn abs_traversal(e: &E, src: &doze_hcl::syntax::Source) -> Option<Traversal> {
    let (root, ops): (
        &hcl_edit::Decorated<hcl_edit::Ident>,
        &[hcl_edit::Decorated<Op>],
    ) = match e {
        E::Variable(v) => (v, &[]),
        E::Traversal(t) => match &t.expr {
            E::Variable(v) => (v, &t.operators),
            _ => return None,
        },
        _ => return None,
    };
    let ts = crate::refs::variables(e, src);
    let t = ts.into_iter().next()?;
    (t.root == root.as_str() && t.steps.len() == ops.len()).then_some(t)
}

/// Go's `dependsOnGone`: `depends_on` is refused with the `after` that says the same.
pub(crate) fn depends_on_gone(
    src: &dyn Sources,
    title: &str,
    explicit: &BTreeMap<String, Condition>,
    attr: &Attr<'_>,
    known: &Declared,
) -> Error {
    let mut items = Vec::new();
    for (name, c) in explicit {
        let Some(kind) = known.kind.get(name) else {
            return pos_err(
                src,
                &At::new(attr.file, attr.range),
                format!(
                    "{title}: depends_on names {}, and nothing is declared by that name",
                    go_quote(name)
                ),
                format!(
                    "depends_on is now after = [module.<name>, …]; {}",
                    did_you_mean(name, known)
                ),
            );
        };
        let r = format!("{}.{name}", kind.as_str());
        items.push(match c {
            Condition::Started => format!("started({r})"),
            Condition::Lazy => format!("lazy({r})"),
            Condition::Healthy => r,
        });
    }
    pos_err(
        src,
        &At::new(attr.file, attr.name_range),
        "`depends_on` is now `after`",
        format!(
            "write: after = [{}] (a bare reference waits until it is ready; started(…) until it runs)",
            items.join(", ")
        ),
    )
}

fn did_you_mean(name: &str, known: &Declared) -> String {
    let mut best: Option<(String, usize)> = None;
    for (n, k) in &known.kind {
        let d = levenshtein(name, n);
        if d < best.as_ref().map_or(4, |b| b.1) {
            best = Some((format!("{}.{n}", k.as_str()), d));
        }
    }
    if let Some((b, _)) = best {
        return format!("did you mean {b}?");
    }
    let mut all: Vec<String> = known
        .kind
        .iter()
        .map(|(n, k)| format!("{}.{n}", k.as_str()))
        .collect();
    all.sort();
    format!("declared: {}", all.join(", "))
}
