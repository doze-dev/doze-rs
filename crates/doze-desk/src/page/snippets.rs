//! A module's connect line in the language you are in: the same URL, rendered several ways (the Go page's
//! `Snippets`).

use super::instance::InstancePage;

/// Each language's name and the lines to paste; empty when the module has no URL or address.
pub fn snippets(p: &InstancePage) -> Vec<(&'static str, String)> {
    let row = p
        .connect
        .iter()
        .find(|r| r.k == "URL")
        .or_else(|| p.connect.iter().find(|r| r.k == "Address"));
    let Some(row) = row else {
        return Vec::new();
    };
    let url = if row.copy.is_empty() {
        &row.v
    } else {
        &row.copy
    };
    if url.is_empty() {
        return Vec::new();
    }
    let env = p
        .connect
        .iter()
        .find(|r| r.k == "Env var")
        .and_then(|r| r.v.split(['=', ' ']).next())
        .filter(|e| !e.is_empty())
        .unwrap_or("URL");
    let mut out = vec![
        (
            "Node",
            format!(
                "// {env} is already set when doze starts your app.\n// From a terminal: eval \"$(doze env)\"\nconst url = process.env.{env} ?? \"{url}\""
            ),
        ),
        (
            "Python",
            format!(
                "import os\n# {env} is already set when doze starts your app.\nurl = os.environ.get(\"{env}\", \"{url}\")"
            ),
        ),
        (
            "Go",
            format!(
                "// {env} is already set when doze starts your app.\nurl := cmp.Or(os.Getenv(\"{env}\"), \"{url}\")"
            ),
        ),
        (
            "Shell",
            format!("export {env}={url}\n# or everything at once: eval \"$(doze env)\""),
        ),
        (".env", format!("{env}={url}")),
    ];
    // TODO(module contract): the psql and redis-cli lines are module-specific. They switch to the client the module
    // declares (manifest `client`, through the SDK and protocol) once that field lands; until then they stay, for
    // parity with Go. Nothing else here may know a module.
    if url.starts_with("postgres") {
        out.push(("psql", format!("psql \"{url}\"")));
    }
    if url.starts_with("redis") {
        out.push(("redis-cli", format!("redis-cli -u \"{url}\"")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::snippets;
    use crate::page::instance::InstancePage;
    use crate::page::parts::Kv;

    #[test]
    fn a_postgres_url_in_every_language_and_psql() {
        let p = InstancePage {
            connect: vec![
                Kv {
                    k: "URL".into(),
                    v: "postgres://x@db:5432/x".into(),
                    copy: "postgres://x@db:5432/x".into(),
                    ..Kv::default()
                },
                Kv {
                    k: "Env var".into(),
                    v: "DATABASE_URL".into(),
                    ..Kv::default()
                },
            ],
            ..InstancePage::default()
        };
        let s = snippets(&p);
        let names: Vec<&str> = s.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["Node", "Python", "Go", "Shell", ".env", "psql"]);
        assert_eq!(s[4].1, "DATABASE_URL=postgres://x@db:5432/x");
        assert!(snippets(&InstancePage::default()).is_empty());
    }
}
