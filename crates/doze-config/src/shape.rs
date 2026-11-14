//! The shape pass's stand-ins: a module that is not resolved still publishes the attributes every
//! module has, and "" for any other attribute something reads (Go's `standInAttributes`). A variable read from its
//! `env` (`module.db.env.DATABASE_URL`) is there too, as "": Go's stand-in had an empty `env`, so such a read failed the
//! shape pass, and the Rust CLI reads every doze.hcl through it.

use crate::files::Files;
use crate::model::Instance;
use crate::refs::walk_body;
use doze_hcl::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Every `module.<name>.<attr>` the files read, by module name; `module.<name>.env.<VAR>` as `env.<VAR>`.
pub(crate) fn referenced_module_attrs(files: &Files) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in &files.files {
        let mut ts = Vec::new();
        walk_body(&f.body, &f.src, true, &mut ts);
        for t in ts.into_iter().filter(|t| t.root == "module") {
            if let (Some(name), Some(attr)) = (t.name(), t.attr()) {
                let read = match t.steps.get(2) {
                    Some(crate::refs::Step::Attr(v) | crate::refs::Step::Index(v))
                        if attr == "env" =>
                    {
                        format!("env.{v}")
                    }
                    _ => attr.to_owned(),
                };
                out.entry(name.to_owned()).or_default().insert(read);
            }
        }
    }
    out
}

/// A module's attributes before it is resolved.
pub(crate) fn stand_in(inst: &Instance, used: Option<&BTreeSet<String>>) -> Value {
    let s = Value::string;
    let mut m = BTreeMap::from([
        ("name".to_owned(), s(&inst.name)),
        ("engine".to_owned(), s(&inst.ty)),
        ("address".to_owned(), s(&format!("127.0.0.1:{}", inst.port))),
        ("host".to_owned(), s("127.0.0.1")),
        ("port".to_owned(), Value::int(i64::from(inst.port))),
        ("url".to_owned(), s("")),
        ("env_var".to_owned(), s("")),
        ("socket".to_owned(), s("")),
    ]);
    let mut env = BTreeMap::new();
    for a in used.into_iter().flatten() {
        match a.strip_prefix("env.") {
            Some(var) => {
                env.insert(var.to_owned(), s(""));
            }
            None => {
                m.entry(a.clone()).or_insert_with(|| s(""));
            }
        }
    }
    m.insert("env".to_owned(), Value::Object(env));
    Value::Object(m)
}
