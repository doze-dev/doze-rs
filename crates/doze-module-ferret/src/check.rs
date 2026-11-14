//! What MongoDB (or FerretDB) would refuse at converge, or what Go accepted and then got wrong, refused before boot at
//! its line in doze.hcl.

use crate::{Collection, FerretSpec, Index, json};
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::Instance;
use doze_module::proto::msg::value::PathSeg;
use std::path::Path;

/// FERRETDB_* settings doze sets itself (Go's list, and telemetry is left to the user).
pub(crate) const OWNED: &[&str] = &[
    "postgresql_url",
    "listen_unix",
    "listen_addr",
    "listen_tls",
    "debug_addr",
    "state_dir",
    "auth",
];

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn index(i: usize) -> PathSeg {
    PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX))
}

fn at(path: Vec<PathSeg>, message: String, hint: Option<&str>) -> Diagnostic {
    Diagnostic {
        path,
        message,
        hint: hint.map(str::to_owned),
    }
}

/// Everything wrong with the block, each at its path.
pub(crate) fn check(s: &FerretSpec, inst: &Instance) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let v = inst.version.trim_start_matches('v');
    if !(v == "2" || v.starts_with("2.")) {
        out.push(at(
            vec![],
            format!(
                "ferret runs FerretDB 2.x, and version {:?} is not one",
                inst.version
            ),
            Some("version = \"2.7\" (only FerretDB 2 speaks to the DocumentDB backend)"),
        ));
    }
    settings(s, &mut out);
    for (i, d) in s.database.iter().enumerate() {
        let here = vec![key("database"), index(i)];
        if let Some(why) = database_name(&d.name) {
            out.push(at(
                here.clone(),
                format!("database {:?} {why}", d.name),
                None,
            ));
        }
        for (j, c) in d.collection.iter().enumerate() {
            let mut p = here.clone();
            p.extend([key("collection"), index(j)]);
            collection(c, &d.name, p, inst, &mut out);
        }
    }
    users(s, &mut out);
    out
}

/// A setting becomes the variable `FERRETDB_<KEY>`: a key doze sets is refused, and so is one that is not a name.
fn settings(s: &FerretSpec, out: &mut Vec<Diagnostic>) {
    for k in s.settings.keys() {
        let here = vec![key("settings"), key(k)];
        if OWNED.iter().any(|o| o.eq_ignore_ascii_case(k)) {
            let msg = format!("FERRETDB_{} is doze's to set", k.to_ascii_uppercase());
            out.push(at(
                here,
                msg,
                Some("doze sets where FerretDB listens, its backend, its state and auth"),
            ));
        } else if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            let msg = format!("{k:?} is not a FerretDB setting name");
            out.push(at(
                here,
                msg,
                Some("letters, digits and underscores, e.g. log_level"),
            ));
        }
    }
}

/// Why MongoDB would refuse `name` for a database, if it would.
fn database_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("needs a name");
    }
    if name.len() >= 64 {
        return Some("is too long (MongoDB allows 63 bytes)");
    }
    if name
        .chars()
        .any(|c| "/\\. \"$*<>:|?".contains(c) || c == '\0')
    {
        return Some("may not hold / \\ . space \" $ * < > : | ? (MongoDB's rule)");
    }
    None
}

/// Why MongoDB would refuse `name` for a collection, if it would.
fn collection_name(name: &str, db: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("needs a name");
    }
    if name.contains('$') || name.contains('\0') {
        return Some("may not hold $ (MongoDB's rule)");
    }
    if name.starts_with("system.") {
        return Some("is in the system. namespace, which MongoDB keeps for itself");
    }
    if db.len() + 1 + name.len() > 255 {
        return Some("makes a namespace over 255 bytes");
    }
    None
}

fn collection(
    c: &Collection,
    db: &str,
    here: Vec<PathSeg>,
    inst: &Instance,
    out: &mut Vec<Diagnostic>,
) {
    if let Some(why) = collection_name(&c.name, db) {
        out.push(at(
            here.clone(),
            format!("collection {:?} {why}", c.name),
            None,
        ));
    }
    if let Some(seed) = &c.seed {
        let mut p = here.clone();
        p.push(key("seed"));
        if let Err(why) = read_seed(seed, &inst.dir) {
            out.push(at(
                p,
                why,
                Some(
                    "a JSON file holding an array of documents, relative to the doze.hcl's folder",
                ),
            ));
        }
    }
    for (i, ix) in c.index.iter().enumerate() {
        let mut p = here.clone();
        p.extend([key("index"), index(i)]);
        index_rules(ix, p, out);
    }
}

fn index_rules(ix: &Index, here: Vec<PathSeg>, out: &mut Vec<Diagnostic>) {
    let mut keys = here.clone();
    keys.push(key("keys"));
    if ix.keys.is_empty() {
        out.push(at(
            keys.clone(),
            "an index needs at least one key".into(),
            Some("keys = { sku = 1 }"),
        ));
    }
    for (f, dir) in &ix.keys {
        let mut p = keys.clone();
        p.push(key(f));
        if *dir != 1 && *dir != -1 {
            out.push(at(
                p.clone(),
                format!("key {f:?} must be 1 or -1, not {dir}"),
                Some("1 ascending, -1 descending"),
            ));
        }
        if f.is_empty() || f.starts_with('$') {
            out.push(at(
                p,
                format!("{f:?} is not a field MongoDB can index"),
                None,
            ));
        }
    }
    if ix.name.as_deref() == Some("") {
        let mut p = here;
        p.push(key("name"));
        out.push(at(
            p,
            "an index name may not be empty".into(),
            Some("leave it out to have one made from the keys"),
        ));
    }
}

/// The seed file's documents: the path relative to the block's folder, read and parsed now so a missing or broken file
/// is said at its line, not at the first converge.
pub(crate) fn read_seed(seed: &str, dir: &str) -> Result<Vec<crate::bson::Doc>, String> {
    let file = seed_path(seed, dir);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read seed {}: {e}", file.display()))?;
    json::documents(&text).map_err(|why| format!("seed {}: {why}", file.display()))
}

/// Where the seed file is: as given when absolute, else under the block's folder.
pub(crate) fn seed_path(seed: &str, dir: &str) -> std::path::PathBuf {
    let p = Path::new(seed);
    if p.is_absolute() || dir.is_empty() {
        p.to_path_buf()
    } else {
        Path::new(dir).join(p)
    }
}

/// Authentication, and the users it needs, are refused: with `auth` on, the gateway refuses doze's own converge
/// (`createUser requires authentication`, proven on FerretDB 2.7), so the instance could never be set up. Go's
/// scaffold accepted both and then failed at boot, or ignored the users without a word.
fn users(s: &FerretSpec, out: &mut Vec<Diagnostic>) {
    if s.auth {
        let msg = "ferret does not support authentication yet: with auth on, FerretDB refuses doze's own setup of the \
                   instance (createUser requires authentication)";
        out.push(at(vec![key("auth")], msg.into(), Some("leave auth out")));
    }
    for (i, u) in s.user.iter().enumerate() {
        let msg = format!(
            "user {:?} needs authentication, which ferret does not support yet",
            u.name
        );
        out.push(at(
            vec![key("user"), index(i)],
            msg,
            Some("remove the user block"),
        ));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn names_mongodb_refuses_are_said() {
        assert!(database_name("shop").is_none());
        for bad in ["", "a.b", "a b", "a/b", "a$b", &"x".repeat(64)] {
            assert!(database_name(bad).is_some(), "{bad:?}");
        }
        assert!(collection_name("orders", "shop").is_none());
        assert!(collection_name("a.b", "shop").is_none());
        for bad in ["", "a$", "system.users"] {
            assert!(collection_name(bad, "shop").is_some(), "{bad:?}");
        }
        assert!(collection_name(&"x".repeat(251), "shop").is_some());
    }

    #[test]
    fn a_relative_seed_is_under_the_blocks_folder() {
        assert_eq!(seed_path("s.json", "/p"), Path::new("/p/s.json"));
        assert_eq!(seed_path("/abs/s.json", "/p"), Path::new("/abs/s.json"));
        assert_eq!(seed_path("s.json", ""), Path::new("s.json"));
    }
}
