//! The blocks `doze init` and `doze add` write, as Go's detector renders them, laid out by doze-hcl's formatter (Go
//! passes the scaffold through `hclwrite.Format`).

use crate::services::{self, Recipe};
use crate::{Declared, Found};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// A string as an HCL literal: Go's `%q` where that is valid HCL, `\u` escapes for other control characters, and
/// `${` / `%{` doubled so a command line is never read as a template.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' | '%' if chars.peek() == Some(&'{') => {
                out.push(c);
                out.push(c);
            }
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A version as written: a bare number when it is one (`version = 9`), else quoted.
fn version_literal(v: &str) -> String {
    if !v.is_empty()
        && v.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !v.starts_with('.')
        && !v.ends_with('.')
    {
        v.to_owned()
    } else {
        quote(v)
    }
}

/// A module block (Go's `moduleBlock`).
pub fn render_module(module: &str, name: &str, version: &str, port: u16) -> String {
    let mut b = format!("module {module} {} {{\n", quote(name));
    if !version.is_empty() {
        let _ = writeln!(b, "  version = {}", version_literal(version));
    }
    if port > 0 {
        let _ = writeln!(b, "  port    = {port}");
    }
    b.push('}');
    b
}

const DOWNLOAD_NOTE: &str =
    "  # fetched once, pinned in doze.lock by its checksum, and run from the archive\n";

/// A `service` block named `name`, run the way `kind`'s recipe says (Go's `ServiceBlock`): the binary `found` on the
/// PATH when there is one and a `run` line is known; else a container (`image`, or the recipe's); else a download with
/// the address left to fill in.
pub fn service_block(name: &str, kind: &str, port: u16, found: &str, image: &str) -> String {
    let r = services::recipe(kind);
    let port = if port == 0 {
        r.map_or(0, |r| r.port)
    } else {
        port
    };
    let run = r.map(|r| r.run).unwrap_or_default();
    let image = if image.is_empty() {
        r.map(|r| r.image).unwrap_or_default()
    } else {
        image
    };
    let mut b = format!("service {} {{\n", quote(name));
    if !found.is_empty() && !run.is_empty() {
        let _ = writeln!(b, "  run  = {}", quote(run));
    } else if !image.is_empty() {
        let cport = r
            .map(|r| r.container_port)
            .filter(|p| *p > 0)
            .unwrap_or(port);
        b.push_str(
            "  # a container: a container is just a command, and `run` stays in the foreground\n",
        );
        let cmd = format!("docker run --rm -p $HOST:$PORT:{cport} {image}");
        let _ = writeln!(b, "  run  = {}", quote(&cmd));
    } else {
        b.push_str(DOWNLOAD_NOTE);
        let _ = writeln!(
            b,
            "  download = \"https://example.com/{kind}-darwin-arm64.tar.gz\" # fill this in"
        );
        let _ = writeln!(
            b,
            "  run      = {}",
            quote(&format!("./{kind} --port $PORT"))
        );
    }
    if port > 0 {
        let _ = writeln!(b, "  port = {port}");
    }
    b.push('}');
    b
}

/// A `service` that is fetched, its address left for the person to fill in (Go's `DownloadBlock`).
pub fn download_block(name: &str, kind: &str, port: u16) -> String {
    let port = if port == 0 {
        services::recipe(kind).map_or(0, |r| r.port)
    } else {
        port
    };
    let mut b = format!("service {} {{\n{DOWNLOAD_NOTE}", quote(name));
    let _ = writeln!(
        b,
        "  download = \"https://example.com/{kind}-darwin-arm64.tar.gz\" # fill this in"
    );
    let _ = writeln!(
        b,
        "  run      = {}",
        quote(&format!("./{kind} --port $PORT"))
    );
    if port > 0 {
        let _ = writeln!(b, "  port     = {port}");
    }
    b.push('}');
    b
}

/// How an app conventionally reads a module, from the variables the module declares: the first that wants a URL,
/// else the first that wants an address (by name). Go kept a table per engine; a module that declares no variable
/// has no wiring line.
pub fn wiring(d: &Declared) -> Option<(String, String)> {
    let env = &d.detect.env;
    env.iter()
        .find(|(_, a)| *a == "url")
        .or_else(|| env.iter().next())
        .map(|(e, a)| (e.clone(), a.clone()))
}

/// The doze.hcl for what was found (Go's `Scaffold`): the stack, each module, service and task with the reason it
/// was found, then each app with its port and its environment wired to what it reads. `look_path` finds a binary on
/// the PATH (empty when it is not there). Laid out as hclwrite lays it out.
pub fn scaffold(f: &Found, look_path: &dyn Fn(&str) -> String) -> String {
    let mut b = format!("stack = {}\n", quote(&f.stack));
    for m in &f.modules {
        let _ = write!(
            b,
            "\n# {}\n{}\n",
            m.why,
            render_module(&m.module, &m.name, &m.version, m.port)
        );
    }
    for s in &f.services {
        let found = services::recipe(&s.name)
            .filter(|r| !r.binary.is_empty())
            .map(|r| look_path(r.binary))
            .unwrap_or_default();
        let _ = write!(
            b,
            "\n# {}\n{}\n",
            s.why,
            service_block(&s.name, &s.name, s.port, &found, &s.image)
        );
    }
    for t in &f.tasks {
        let _ = write!(
            b,
            "\n# {}\ntask {} {{\n  run = {}\n",
            t.why,
            quote(&t.name),
            quote(&t.run)
        );
        if t.manual {
            b.push_str("  manual = true\n");
        }
        b.push_str("}\n");
    }
    apps(f, &mut b);
    String::from_utf8_lossy(&doze_hcl::fmt::format(b.as_bytes())).into_owned()
}

fn apps(f: &Found, b: &mut String) {
    let mut used: BTreeSet<u16> = f.apps.iter().map(|a| a.port).collect();
    let mut next = 8080u16;
    let migrations: Vec<&str> = ["migrate", "migrate-up", "db-migrate", "migrations"]
        .into_iter()
        .filter(|n| f.tasks.iter().any(|t| t.name == *n))
        .collect();
    for a in &f.apps {
        let _ = write!(
            b,
            "\n# {}\napp {} {{\n  run = {}\n",
            a.why,
            quote(&a.name),
            quote(&a.run)
        );
        let mut port = a.port;
        if port == 0 && a.reads_port {
            while used.contains(&next) {
                next = next.saturating_add(1);
            }
            port = next;
            used.insert(port);
            b.push_str("  # it reads $PORT, so it is given a port — change it if you like\n");
        }
        if port > 0 {
            let _ = writeln!(b, "  port = {port}");
        }
        if let Some(env) = &f.env_file {
            let _ = writeln!(b, "  env_file = {}", quote(env));
        }
        let mut lines: Vec<String> = a.uses.iter().map(|u| env_line(f, u)).collect();
        lines.sort();
        if !lines.is_empty() {
            let _ = write!(b, "\n  env = {{\n{}\n  }}\n", lines.join("\n"));
        }
        // An app that reads a database waits for the project's migration.
        let reads_db = a
            .uses
            .iter()
            .any(|u| f.modules.iter().any(|m| m.module == u.kind && m.database));
        if reads_db && !migrations.is_empty() {
            let refs: Vec<String> = migrations.iter().map(|n| format!("task.{n}")).collect();
            let _ = write!(b, "\n  after = [{}]\n", refs.join(", "));
        }
        b.push_str("}\n");
    }
}

fn env_line(f: &Found, u: &crate::Use) -> String {
    if let Some(m) = f.modules.iter().find(|m| m.module == u.kind) {
        return format!("    {} = module.{}.{}", u.env, m.name, u.attr);
    }
    let address = format!("service.{}.address", u.kind);
    let scheme = services::recipe(&u.kind)
        .map(|r: &Recipe| r.url_scheme)
        .unwrap_or_default();
    if scheme.is_empty() {
        format!("    {} = {address}", u.env)
    } else {
        format!("    {} = \"{scheme}${{{address}}}\"", u.env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_is_go_s_and_never_a_template() {
        assert_eq!(quote("go run ./cmd/api"), "\"go run ./cmd/api\"");
        assert_eq!(
            quote("a \"b\" \\ ${X} %{if}"),
            "\"a \\\"b\\\" \\\\ $${X} %%{if}\""
        );
        assert_eq!(quote("\u{7}"), "\"\\u0007\"");
    }

    #[test]
    fn a_module_block_is_go_s() {
        assert_eq!(
            render_module("postgres", "db", "18", 5432),
            "module postgres \"db\" {\n  version = 18\n  port    = 5432\n}"
        );
        assert_eq!(
            render_module("aws", "cloud", "", 4566),
            "module aws \"cloud\" {\n  port    = 4566\n}"
        );
    }
}
