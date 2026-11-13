//! "Also on this machine": services other doze programs (a standalone doze-aws or doze-kafka) run here, read from
//! the shared names file (`$DOZE_HOME/names.json`, the names crate's format). Read only: the file is never written
//! from here, so the Library cannot disturb a name another program holds.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A service another doze program runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Other {
    pub name: String,
    /// `doze-aws`, `doze-kafka`.
    pub owner: String,
    pub pid: i64,
    /// Where to open its console.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    pid: i64,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    target: String,
}

/// The names the front door holds the http port with (the names crate's `_ingress`).
const INGRESS: &str = "_ingress";

/// What other doze programs run, by name, from the names file under `home`; their processes alive.
pub fn others(home: &Path) -> Vec<Other> {
    let Ok(raw) = std::fs::read(home.join("names.json")) else {
        return Vec::new();
    };
    let Ok(m) = serde_json::from_slice::<BTreeMap<String, Entry>>(&raw) else {
        return Vec::new();
    };
    let alive = |e: &Entry| u32::try_from(e.pid).is_ok_and(doze_os::process::alive);
    let front = m
        .get(INGRESS)
        .filter(|e| alive(e))
        .map(|e| e.target.as_str());
    m.iter()
        .filter(|(host, e)| {
            !host.starts_with('_')
                && !e.owner.is_empty()
                && e.owner != "doze"
                && (host.as_str() == "doze" || host.ends_with(".doze"))
                && !e.target.is_empty()
                && alive(e)
        })
        .map(|(host, e)| Other {
            name: host.clone(),
            owner: e.owner.clone(),
            pid: e.pid,
            url: url_for(host, &e.target, front),
        })
        .collect()
}

/// Port-less while a doze program holds the front door on 80, else with the backend's port.
fn url_for(host: &str, target: &str, front: Option<&str>) -> String {
    let port = |t: &str| t.rsplit_once(':').map(|(_, p)| p.to_owned());
    match front {
        Some(f) => match port(f) {
            Some(p) if p != "80" => format!("http://{host}:{p}"),
            _ => format!("http://{host}"),
        },
        None => port(target).map_or_else(String::new, |p| format!("http://{host}:{p}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{others, url_for};

    #[test]
    fn reads_the_names_of_other_programs() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let json = format!(
            r#"{{"_format":{{"pid":{me},"owner":"doze","format":2}},
               "aws.harbour.doze":{{"ip":"127.0.0.2","pid":{me},"owner":"doze-aws","target":"127.0.0.2:4566"}},
               "api.shop.doze":{{"ip":"127.0.0.3","pid":{me},"owner":"doze","target":"127.0.0.3:8080"}},
               "dead.doze":{{"pid":999999999,"owner":"doze-kafka","target":"127.0.0.4:9092"}}}}"#
        );
        std::fs::write(dir.path().join("names.json"), json).unwrap();
        let o = others(dir.path());
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].owner, "doze-aws");
        assert_eq!(o[0].url, "http://aws.harbour.doze:4566");
        assert!(others(&dir.path().join("none")).is_empty());
        assert_eq!(
            url_for("a.doze", "x:1", Some("127.0.0.1:80")),
            "http://a.doze"
        );
    }
}
