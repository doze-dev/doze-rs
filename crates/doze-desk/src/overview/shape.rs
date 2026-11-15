//! A stopped stack's shape: what its doze.hcl declares, with no daemon to ask. Two sources, each for what it knows:
//!
//! - `doze --json status` in the stack's folder, the CLI's machine contract (PLAN.md §4.1): every declared instance,
//!   its kind, engine, version, whether it is enabled, its address, what it depends on; and, when the file does not
//!   load, the loader's words. The Rust CLI keeps the same envelope, so this does not change when it replaces Go's.
//! - The file itself ([`super::scan`]): the run lines (the Map's language badges, the pane's command), the order the
//!   references are written in, ports and domains, and the Grid's quotes. It is read even when the CLI is not there
//!   or cannot load the file, so a broken or offline stack still shows its rows.

use std::path::Path;

use serde::Deserialize;

use super::refs::{HclFile, scan};

/// One declared instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decl {
    pub name: String,
    /// `app`, `module`, `service` or `task`.
    pub kind: String,
    /// A module's engine (`postgres`); `process` for an app or a service.
    pub engine: String,
    pub version: String,
    pub enabled: bool,
    /// What it refers to, in the file's order.
    pub needs: Vec<String>,
    pub run: String,
    pub port: Option<u32>,
    pub domains: Vec<String>,
    /// Where the CLI says it listens (`127.0.0.1:5432`), when it said.
    pub endpoint: String,
}

/// A stopped stack, read from its doze.hcl.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shape {
    /// `stack = "…"`, else empty.
    pub stack: String,
    pub decls: Vec<Decl>,
    /// The loader's words, when the file does not load.
    pub error: Option<String>,
    /// The file as it was read, for the Grid's quotes.
    pub file: HclFile,
}

/// `doze --json status` said something this reader could not take.
#[derive(Debug, thiserror::Error)]
#[error("doze status --json: {0}")]
pub struct ShapeError(String);

#[derive(Debug, Default, Deserialize)]
struct Envelope {
    #[serde(default)]
    data: Option<StatusData>,
    #[serde(default)]
    error: Option<EnvelopeError>,
}

#[derive(Debug, Default, Deserialize)]
struct EnvelopeError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct StatusData {
    #[serde(default)]
    services: Vec<Service>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Service {
    name: String,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    group: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    depends_on: Vec<String>,
}

fn yes() -> bool {
    true
}

/// What `doze --json status` printed, read: its services, and the loader's words when the file does not load.
pub fn parse_status(json: &str) -> Result<(Vec<Decl>, Option<String>), ShapeError> {
    let env: Envelope = serde_json::from_str(json).map_err(|e| ShapeError(e.to_string()))?;
    let error = env
        .error
        .filter(|e| e.code == "config_invalid" || e.message.contains("Error: "))
        .map(|e| e.message);
    let decls = env
        .data
        .map(|d| d.services)
        .unwrap_or_default()
        .into_iter()
        .map(|s| Decl {
            kind: if s.group.is_empty() {
                "module".into()
            } else {
                s.group
            },
            engine: s.engine,
            version: s.version,
            enabled: s.enabled,
            needs: s.depends_on,
            endpoint: s.endpoint,
            name: s.name,
            ..Decl::default()
        })
        .collect();
    Ok((decls, error))
}

impl Shape {
    /// The shape of the stack in `dir`: its files, and what `doze --json status` said there, if it was asked.
    pub fn read(dir: &Path, status: Option<&str>) -> Shape {
        let mut file = std::fs::read_to_string(dir.join("doze.hcl"))
            .map(|s| scan(&s))
            .unwrap_or_default();
        if let Some(refs) = super::refs::refs_in(dir) {
            file.refs = refs;
        }
        let (listed, error) = status
            .and_then(|s| parse_status(s).ok())
            .unwrap_or_default();
        Shape::merge(file, listed, error)
    }

    /// Each declared instance from what the CLI listed, filled in from the file; the file's alone when the CLI listed
    /// none.
    pub fn merge(file: HclFile, listed: Vec<Decl>, error: Option<String>) -> Shape {
        let from_file = |b: &super::refs::Block| Decl {
            name: b.name.clone(),
            kind: b.kind.clone(),
            engine: if b.kind == "module" {
                b.ty.clone()
            } else {
                "process".into()
            },
            version: b.version.clone(),
            enabled: b.enabled,
            needs: b.needs.clone(),
            run: b.run.clone(),
            port: b.port,
            domains: b.domains.clone(),
            endpoint: String::new(),
        };
        let decls = if listed.is_empty() {
            file.blocks.iter().map(from_file).collect()
        } else {
            listed
                .into_iter()
                .map(|mut d| {
                    if let Some(b) = file.blocks.iter().find(|b| b.name == d.name) {
                        let f = from_file(b);
                        if !f.needs.is_empty() || d.needs.is_empty() {
                            d.needs = f.needs;
                        }
                        (d.run, d.port, d.domains) = (f.run, f.port, f.domains);
                        if d.version.is_empty() {
                            d.version = f.version;
                        }
                    }
                    d
                })
                .collect()
        };
        Shape {
            stack: file.stack.clone(),
            decls,
            error,
            file,
        }
    }
}

impl Shape {
    /// The declared instances as a daemon would list them (asleep, nothing running), so a page can draw a stopped
    /// stack the way it will once up: the Map, the rows.
    pub fn instances(&self) -> Vec<doze_control::InstanceView> {
        self.decls.iter().map(super::stopped::view_of).collect()
    }
}

impl Decl {
    /// Where it answers once the stack is up, written as a running stack writes it (Go `addressWhenUp`): its own
    /// domain over https; an app's or a service's `.doze` name as a web address; a module's name and port.
    pub fn address_when_up(&self, stack: &str) -> String {
        if let Some(d) = self.domains.first() {
            return format!("https://{}", d.strip_prefix("*.").unwrap_or(d));
        }
        if stack.is_empty() || self.kind == "task" {
            return String::new();
        }
        let name = format!("{}.{stack}.doze", self.name);
        let port = self.port.or_else(|| {
            self.endpoint
                .rsplit_once(':')
                .and_then(|(_, p)| p.parse().ok())
        });
        let web = self.kind == "app" || self.kind == "service";
        match port {
            Some(p) if web => format!("http://{name}:{p}"),
            Some(p) => format!("{name}:{p}"),
            None => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Shape, parse_status};

    const OFFLINE: &str = r#"{"schema":"doze/v1","command":"doze status","ok":false,"data":{"daemon":{"running":false},"services":[{"name":"api","engine":"process","group":"app","state":"asleep","enabled":true,"conns":0,"memBytes":0,"cpuPercent":0},{"name":"db","engine":"postgres","version":"17","group":"module","state":"asleep","enabled":true,"conns":0,"memBytes":0,"cpuPercent":0}]},"error":{"code":"config_invalid","message":"the stack's config has a mistake —\n\nError: no module called \"postgres\"\n\n  on doze.hcl line 24, in module \"postgres\" \"db\":\n  24: module postgres \"db\" { version = 17 }","exit":5}}"#;

    #[test]
    fn reads_what_the_cli_says() {
        let (decls, error) = parse_status(OFFLINE).unwrap();
        assert_eq!(decls.len(), 2);
        assert_eq!(
            (decls[1].engine.as_str(), decls[1].version.as_str()),
            ("postgres", "17")
        );
        assert!(error.unwrap().contains("Error: no module called"));
        assert!(parse_status("not json").is_err());
    }

    #[test]
    fn the_file_fills_in_what_the_cli_does_not_say() {
        let file = super::scan(include_str!("../../testdata/go/hcl/acme.hcl"));
        let (decls, _) = parse_status(OFFLINE).unwrap();
        let s = Shape::merge(file, decls, None);
        let api = &s.decls[0];
        assert_eq!(api.run, "go run ./cmd/api");
        assert_eq!(api.needs, ["db", "cache"]);
        assert_eq!(api.address_when_up("acme"), "http://api.acme.doze:8080");
        assert_eq!(s.decls[1].address_when_up("acme"), "db.acme.doze");
    }
}
