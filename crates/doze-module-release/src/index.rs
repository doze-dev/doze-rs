//! The module's release index (schema 1), the file doze-registry's `publish.mjs` signs. The same rules as the Go
//! `modtool`: a published (version, target) artifact never changes; the stable channel is the highest version; a
//! mutation drops any signature. Protocol-1 releases a Go build published stay untouched, so the Go doze keeps
//! selecting them (it only takes releases whose protocol is its own).

use std::collections::BTreeMap;
use yaml_rust2::yaml::{Hash, Yaml};
use yaml_rust2::{YamlEmitter, YamlLoader};

pub(crate) const SCHEMA: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Artifact {
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Release {
    pub(crate) protocol: i64,
    pub(crate) engines: Vec<String>,
    pub(crate) artifacts: BTreeMap<String, Artifact>,
    pub(crate) doze: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Index {
    pub(crate) module: String,
    pub(crate) namespace: String,
    pub(crate) releases: BTreeMap<String, Release>,
    pub(crate) channels: BTreeMap<String, String>,
}

fn s(y: &Yaml) -> String {
    match y {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        _ => String::new(),
    }
}

fn get<'a>(h: &'a Hash, k: &str) -> &'a Yaml {
    h.get(&Yaml::String(k.to_string()))
        .unwrap_or(&Yaml::BadValue)
}

fn map_of<T>(y: &Yaml, f: impl Fn(&Yaml) -> T) -> BTreeMap<String, T> {
    y.as_hash()
        .map(|h| h.iter().map(|(k, v)| (s(k), f(v))).collect())
        .unwrap_or_default()
}

/// Reads an index. A file of another schema is refused (the Go tool discarded it; publishing over it is a decision).
pub(crate) fn parse(text: &str) -> Result<Index, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| format!("index.yaml: {e}"))?;
    let h = docs
        .first()
        .and_then(Yaml::as_hash)
        .ok_or("index.yaml: not a mapping")?;
    if get(h, "schema").as_i64() != Some(SCHEMA) {
        return Err(format!("index.yaml is not schema {SCHEMA}"));
    }
    let release = |r: &Yaml| {
        let Some(r) = r.as_hash() else {
            return Release::default();
        };
        Release {
            protocol: get(r, "protocol").as_i64().unwrap_or(0),
            engines: get(r, "engines")
                .as_vec()
                .map(|v| v.iter().map(s).collect())
                .unwrap_or_default(),
            artifacts: map_of(get(r, "artifacts"), |a| {
                let a = a.as_hash();
                let field = |k| a.map(|a| s(get(a, k))).unwrap_or_default();
                Artifact {
                    url: field("url"),
                    sha256: field("sha256"),
                    sig: field("sig"),
                }
            }),
            doze: s(get(r, "doze")),
        }
    };
    Ok(Index {
        module: s(get(h, "module")),
        namespace: s(get(h, "namespace")),
        releases: map_of(get(h, "releases"), release),
        channels: map_of(get(h, "channels"), s),
    })
}

fn key(k: &str) -> Yaml {
    Yaml::String(k.to_string())
}

/// Writes the index in the Go tool's key order (signatures are never written: the registry signs).
pub(crate) fn render(idx: &Index) -> Result<String, String> {
    let mut releases = Hash::new();
    for (v, r) in &idx.releases {
        let mut rel = Hash::new();
        rel.insert(key("protocol"), Yaml::Integer(r.protocol));
        if !r.engines.is_empty() {
            rel.insert(
                key("engines"),
                Yaml::Array(r.engines.iter().map(|e| key(e)).collect()),
            );
        }
        let mut arts = Hash::new();
        for (t, a) in &r.artifacts {
            let mut art = Hash::new();
            art.insert(key("url"), key(&a.url));
            art.insert(key("sha256"), key(&a.sha256));
            if !a.sig.is_empty() {
                art.insert(key("sig"), key(&a.sig));
            }
            arts.insert(key(t), Yaml::Hash(art));
        }
        rel.insert(key("artifacts"), Yaml::Hash(arts));
        if !r.doze.is_empty() {
            rel.insert(key("doze"), key(&r.doze));
        }
        releases.insert(key(v), Yaml::Hash(rel));
    }
    let mut top = Hash::new();
    top.insert(key("schema"), Yaml::Integer(SCHEMA));
    top.insert(key("module"), key(&idx.module));
    top.insert(key("namespace"), key(&idx.namespace));
    top.insert(key("releases"), Yaml::Hash(releases));
    top.insert(
        key("channels"),
        Yaml::Hash(idx.channels.iter().map(|(k, v)| (key(k), key(v))).collect()),
    );
    let mut out = String::new();
    YamlEmitter::new(&mut out)
        .dump(&Yaml::Hash(top))
        .map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

/// Numeric per dotted segment, text where a segment is not a number (the Go `modindex.CompareVersions`).
pub(crate) fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        match (x.next(), y.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(p), Some(q)) => {
                let o = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(m), Ok(n)) => m.cmp(&n),
                    _ => p.cmp(q),
                };
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// What a new artifact adds to the index.
#[derive(Debug, Clone)]
pub(crate) struct Addition {
    pub(crate) version: String,
    pub(crate) target: String,
    pub(crate) archive: String,
    pub(crate) sha256: String,
    pub(crate) protocol: i64,
    pub(crate) engines: Vec<String>,
    pub(crate) min_doze: String,
}

/// Adds an artifact. Refuses to change one already published with other bytes ("bump the module version").
pub(crate) fn add(idx: &mut Index, a: &Addition) -> Result<(), String> {
    let rel = idx.releases.entry(a.version.clone()).or_default();
    if let Some(prev) = rel.artifacts.get(&a.target)
        && !prev.sha256.eq_ignore_ascii_case(&a.sha256)
    {
        return Err(format!(
            "{} {} ({}) is already published with a different sha256: bump the module version instead of rebuilding it",
            idx.module, a.version, a.target
        ));
    }
    rel.protocol = a.protocol;
    rel.engines.clone_from(&a.engines);
    rel.doze.clone_from(&a.min_doze);
    rel.artifacts.insert(
        a.target.clone(),
        Artifact {
            url: a.archive.clone(),
            sha256: a.sha256.to_lowercase(),
            sig: String::new(),
        },
    );
    // Any signature is stale now; the registry signs again.
    rel.artifacts.values_mut().for_each(|art| art.sig.clear());
    let stable = idx
        .releases
        .keys()
        .max_by(|p, q| compare_versions(p, q))
        .cloned()
        .unwrap_or_else(|| a.version.clone());
    idx.channels.insert("stable".into(), stable);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An index as the Go modtool writes it (yaml.v3), with a signed protocol-1 release already published.
    const GO_INDEX: &str = "schema: 1\nmodule: valkey\nnamespace: doze\nreleases:\n    0.2.2:\n        protocol: 1\n        engines:\n            - \"8\"\n            - \"9\"\n        artifacts:\n            aarch64-apple-darwin:\n                url: valkey-plugin-0.2.2-aarch64-apple-darwin.tar.gz\n                sha256: AAAA\n                sig: c2lnbmVk\n        doze: 0.2.0\nchannels:\n    stable: 0.2.2\n";

    fn addition(version: &str, sha: &str) -> Addition {
        Addition {
            version: version.into(),
            target: "x86_64-pc-windows-msvc".into(),
            archive: format!("valkey-plugin-{version}-x86_64-pc-windows-msvc.tar.gz"),
            sha256: sha.into(),
            protocol: 2,
            engines: vec!["8".into(), "9".into()],
            min_doze: "0.3.0".into(),
        }
    }

    #[test]
    fn a_protocol_2_release_joins_a_go_index_without_touching_it() {
        let mut idx = parse(GO_INDEX).unwrap();
        let go_release = idx.releases["0.2.2"].clone();
        add(&mut idx, &addition("0.3.0", "BBBB")).unwrap();
        assert_eq!(
            idx.releases["0.2.2"], go_release,
            "the published Go release must stay as it was, signature and all"
        );
        assert_eq!(idx.releases["0.3.0"].protocol, 2);
        assert_eq!(
            idx.releases["0.3.0"].artifacts["x86_64-pc-windows-msvc"].sha256,
            "bbbb"
        );
        assert_eq!(idx.channels["stable"], "0.3.0");
        assert_eq!(
            parse(&render(&idx).unwrap()).unwrap(),
            idx,
            "what is written must read back the same"
        );
    }

    #[test]
    fn a_published_artifact_never_changes() {
        let mut idx = parse(GO_INDEX).unwrap();
        add(&mut idx, &addition("0.3.0", "bbbb")).unwrap();
        add(&mut idx, &addition("0.3.0", "BBBB")).unwrap();
        let e = add(&mut idx, &addition("0.3.0", "cccc")).unwrap_err();
        assert!(e.contains("bump the module version"), "{e}");
    }

    #[test]
    fn versions_compare_by_number() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Greater);
        assert_eq!(compare_versions("1.2", "1.2.0"), Less);
        assert_eq!(compare_versions("2.0.0", "2.0.0"), Equal);
        assert!(parse("schema: 2\nmodule: x\n").is_err());
    }
}
