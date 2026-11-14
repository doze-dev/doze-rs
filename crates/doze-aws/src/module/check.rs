//! What the stack would refuse, or would quietly get wrong, refused before boot at its line in doze.hcl. Go's module
//! passed it all to the running instance: a reference to a queue nobody declared, a malformed key or JSON, a missing
//! code directory each failed the converge after boot, half the stack made.

use std::collections::BTreeSet;

use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

use super::spec::{AwsSpec, Destination, Index, Table};
use super::template::{key_parts, queue_name};
use super::template_more::code_dir;

pub(super) fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

pub(super) fn idx(i: usize) -> PathSeg {
    PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX))
}

/// What a check found, each at its path.
#[derive(Debug, Default)]
pub(super) struct Found(Vec<Diagnostic>);

impl Found {
    pub(super) fn at(&mut self, path: Vec<PathSeg>, message: String, hint: Option<&str>) {
        self.0.push(Diagnostic {
            path,
            message,
            hint: hint.map(str::to_owned),
        });
    }
}

/// The names each kind of block declares, for references between them.
pub(super) struct Names {
    queues: BTreeSet<String>,
    topics: BTreeSet<String>,
    functions: BTreeSet<String>,
}

impl Names {
    fn of(s: &AwsSpec) -> Names {
        Names {
            queues: s.queue.iter().map(|q| q.name.clone()).collect(),
            topics: s.topic.iter().map(|t| t.name.clone()).collect(),
            functions: s.function.iter().map(|f| f.name.clone()).collect(),
        }
    }

    pub(super) fn has(&self, kind: &str, name: &str) -> bool {
        match kind {
            "queue" => self.queues.contains(name),
            "topic" => self.topics.contains(name),
            "lambda" => self.functions.contains(name),
            _ => false,
        }
    }
}

/// Everything wrong with `s`, each at its path. `dir` is the folder of the file the block is in.
pub(crate) fn check(s: &AwsSpec, dir: &str) -> Vec<Diagnostic> {
    let mut f = Found::default();
    let names = Names::of(s);
    duplicates(s, &mut f);
    queues(s, &names, &mut f);
    for (i, b) in s.bucket.iter().enumerate() {
        for (j, n) in b.notify.iter().enumerate() {
            let here = vec![key("bucket"), idx(i), key("notify"), idx(j)];
            one_target(
                &names,
                &[
                    ("queue", &n.queue),
                    ("topic", &n.topic),
                    ("lambda", &n.lambda),
                ],
                here,
                &mut f,
            );
        }
    }
    for (i, t) in s.topic.iter().enumerate() {
        for (j, sub) in t.subscribe.iter().enumerate() {
            let here = vec![key("topic"), idx(i), key("subscribe"), idx(j)];
            let http = sub.http.as_ref().map(|_| String::new());
            one_target(
                &names,
                &[
                    ("queue", &sub.queue),
                    ("lambda", &sub.lambda),
                    ("http", &http),
                ],
                here.clone(),
                &mut f,
            );
            json(
                sub.filter.as_deref(),
                [here, vec![key("filter")]].concat(),
                &mut f,
            );
        }
    }
    for (i, t) in s.table.iter().enumerate() {
        table(t, vec![key("table"), idx(i)], &mut f);
    }
    functions(s, &names, dir, &mut f);
    super::check_more::rules(s, &names, &mut f);
    super::check_more::rest(s, &mut f);
    f.0
}

/// Two blocks of one kind with one label would be one resource declared twice.
fn duplicates(s: &AwsSpec, f: &mut Found) {
    let kinds: [(&str, Vec<String>); 10] = [
        ("bucket", s.bucket.iter().map(|x| x.name.clone()).collect()),
        ("queue", s.queue.iter().map(queue_name).collect()),
        ("topic", s.topic.iter().map(|x| x.name.clone()).collect()),
        ("table", s.table.iter().map(|x| x.name.clone()).collect()),
        (
            "function",
            s.function.iter().map(|x| x.name.clone()).collect(),
        ),
        ("rule", s.rule.iter().map(|x| x.name.clone()).collect()),
        ("key", s.key.iter().map(|x| x.name.clone()).collect()),
        ("secret", s.secret.iter().map(|x| x.name.clone()).collect()),
        (
            "parameter",
            s.parameter.iter().map(|x| x.name.clone()).collect(),
        ),
        (
            "state_machine",
            s.state_machine.iter().map(|x| x.name.clone()).collect(),
        ),
    ];
    for (kind, labels) in kinds {
        let mut seen = BTreeSet::new();
        for (i, l) in labels.iter().enumerate() {
            if !seen.insert(l) {
                f.at(
                    vec![key(kind), idx(i)],
                    format!("{kind} {l:?} is declared more than once"),
                    None,
                );
            }
        }
    }
}

fn queues(s: &AwsSpec, names: &Names, f: &mut Found) {
    for (i, q) in s.queue.iter().enumerate() {
        let here = |k: &str| vec![key("queue"), idx(i), key(k)];
        if q.content_dedup && !q.fifo {
            f.at(
                here("content_dedup"),
                format!(
                    "queue {:?}: content-based deduplication is for FIFO queues",
                    q.name
                ),
                Some("add fifo = true"),
            );
        }
        match q.dlq.as_deref() {
            Some("auto") => {
                let companion = format!("{}-dlq", q.name.trim_end_matches(".fifo"));
                if names
                    .queues
                    .iter()
                    .any(|n| n.trim_end_matches(".fifo") == companion)
                {
                    f.at(
                        here("dlq"),
                        format!(
                            "dlq = \"auto\" makes {companion:?}, which another queue block declares"
                        ),
                        Some("dlq = the name of that queue"),
                    );
                }
            }
            None => {}
            Some(d) if d == q.name => f.at(
                here("dlq"),
                format!("queue {:?} cannot be its own dead-letter queue", q.name),
                None,
            ),
            Some(d) if !names.queues.contains(d) => f.at(
                here("dlq"),
                format!("no queue {d:?} is declared"),
                Some("\"auto\", or the name of another queue block"),
            ),
            Some(_) => {}
        }
        if q.max_receives.is_some() && q.dlq.is_none() {
            f.at(
                here("max_receives"),
                "max_receives needs a dlq to send messages to".into(),
                Some("add dlq = \"auto\""),
            );
        }
    }
}

/// Exactly one of the targets is set, and what it names is declared.
fn one_target(
    names: &Names,
    targets: &[(&str, &Option<String>)],
    here: Vec<PathSeg>,
    f: &mut Found,
) {
    let set: Vec<(&str, &String)> = targets
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| (*k, v)))
        .collect();
    let words: Vec<&str> = targets.iter().map(|(k, _)| *k).collect();
    match set.as_slice() {
        [(kind, name)] if *kind != "http" && !names.has(kind, name) => {
            f.at(
                [here, vec![key(kind)]].concat(),
                format!("no {kind} {name:?} is declared in this block"),
                None,
            );
        }
        [_] => {}
        _ => f.at(
            here,
            format!("set exactly one of {}", words.join(", ")),
            None,
        ),
    }
}

pub(super) fn json(text: Option<&str>, here: Vec<PathSeg>, f: &mut Found) {
    let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    if let Err(e) = serde_json::from_str::<serde_json::Value>(t) {
        f.at(here, format!("not JSON: {e}"), None);
    }
}

/// A key in the table's shorthand: one or two `name:TYPE`, the type S, N or B.
fn key_ok(k: &str) -> bool {
    let parts = key_parts(k);
    (1..=2).contains(&parts.len())
        && parts.len() == k.split_whitespace().count()
        && parts
            .iter()
            .all(|(n, t)| !n.is_empty() && matches!(t.as_str(), "S" | "N" | "B"))
}

fn table(t: &Table, here: Vec<PathSeg>, f: &mut Found) {
    let hint = Some("\"pk:S\" or \"pk:S sk:N\"; types S, N or B");
    // One attribute, one type: the table and its indexes share their attribute definitions.
    let mut types: Vec<(String, String)> = Vec::new();
    for k in std::iter::once(&t.key).chain(t.gsi.iter().chain(&t.lsi).map(|i| &i.key)) {
        for (n, ty) in key_parts(k) {
            match types.iter().find(|(a, _)| *a == n) {
                Some((_, was)) if *was != ty => f.at(
                    here.clone(),
                    format!("attribute {n:?} is {was} in one key and {ty} in another"),
                    None,
                ),
                Some(_) => {}
                None => types.push((n, ty)),
            }
        }
    }
    if !key_ok(&t.key) {
        f.at(
            [here.clone(), vec![key("key")]].concat(),
            format!("{:?} is not a key", t.key),
            hint,
        );
    }
    let check_index = |kind: &str, i: usize, ix: &Index, f: &mut Found| {
        let at = [here.clone(), vec![key(kind), idx(i)]].concat();
        if !key_ok(&ix.key) {
            f.at(
                [at.clone(), vec![key("key")]].concat(),
                format!("{:?} is not a key", ix.key),
                hint,
            );
        }
        let projection = ix
            .projection
            .as_deref()
            .unwrap_or("ALL")
            .to_ascii_uppercase();
        if !matches!(projection.as_str(), "ALL" | "KEYS_ONLY" | "INCLUDE") {
            f.at(
                [at.clone(), vec![key("projection")]].concat(),
                format!("{projection:?} is not a projection"),
                Some("ALL, KEYS_ONLY or INCLUDE"),
            );
        } else if (projection == "INCLUDE") == ix.include.is_empty() {
            f.at(
                at.clone(),
                "include is for an INCLUDE projection, and one needs it".into(),
                None,
            );
        }
        if kind == "lsi"
            && key_parts(&ix.key).first().map(|p| &p.0) != key_parts(&t.key).first().map(|p| &p.0)
        {
            f.at(
                [at, vec![key("key")]].concat(),
                "a local index has the table's partition key".into(),
                None,
            );
        }
    };
    for (i, ix) in t.gsi.iter().enumerate() {
        check_index("gsi", i, ix, f);
    }
    for (i, ix) in t.lsi.iter().enumerate() {
        check_index("lsi", i, ix, f);
    }
}

fn destination(names: &Names, d: &[Destination], here: Vec<PathSeg>, f: &mut Found) {
    if d.len() > 1 {
        f.at(here.clone(), "declared more than once".into(), None);
    }
    for (i, x) in d.iter().enumerate() {
        let at = [here.clone(), vec![idx(i)]].concat();
        one_target(
            names,
            &[
                ("queue", &x.queue),
                ("topic", &x.topic),
                ("lambda", &x.lambda),
            ],
            at,
            f,
        );
    }
}

fn functions(s: &AwsSpec, names: &Names, dir: &str, f: &mut Found) {
    for (i, x) in s.function.iter().enumerate() {
        let here = |k: &str| vec![key("function"), idx(i), key(k)];
        let code = code_dir(&x.code, dir);
        if !std::path::Path::new(&code).is_dir() {
            f.at(
                here("code"),
                format!("{code} is not a directory"),
                Some("a folder with the function's code, relative to doze.hcl"),
            );
        }
        if x.retries.is_some_and(|r| !(0..=2).contains(&r)) {
            f.at(here("retries"), "retries is 0, 1 or 2".into(), None);
        }
        destination(names, &x.dlq, here("dlq"), f);
        destination(names, &x.on_success, here("on_success"), f);
        destination(names, &x.on_failure, here("on_failure"), f);
        for (j, t) in x.trigger.iter().enumerate() {
            if !names.queues.contains(&t.queue) {
                f.at(
                    [here("trigger"), vec![idx(j), key("queue")]].concat(),
                    format!("no queue {:?} is declared in this block", t.queue),
                    None,
                );
            }
        }
    }
}
