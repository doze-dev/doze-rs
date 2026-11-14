//! The shape pass's stand-ins: a module that is not resolved still publishes the attributes every
//! module has, and "" for any other attribute something reads (Go's `standInAttributes`).

use crate::files::Files;
use crate::model::Instance;
use crate::refs::walk_body;
use doze_hcl::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Every `module.<name>.<attr>` the files read, by module name.
pub(crate) fn referenced_module_attrs(files: &Files) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in &files.files {
        let mut ts = Vec::new();
        walk_body(&f.body, &f.src, true, &mut ts);
        for t in ts.into_iter().filter(|t| t.root == "module") {
            if let (Some(name), Some(attr)) = (t.name(), t.attr()) {
                out.entry(name.to_owned())
                    .or_default()
                    .insert(attr.to_owned());
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
        ("env".to_owned(), Value::Object(BTreeMap::new())),
    ]);
    for a in used.into_iter().flatten() {
        m.entry(a.clone()).or_insert_with(|| s(""));
    }
    Value::Object(m)
}
