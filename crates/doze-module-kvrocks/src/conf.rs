//! kvrocks.conf, written by doze on every boot: one directive per line, `key value`. Every user value lands on a line
//! of its own, so a value that could start another line (a control character) is refused before anything is written.

use crate::KvrocksSpec;
use doze_module::Result;
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::instance::Address;
use doze_module::proto::msg::run::Ready;
use doze_module::proto::msg::value::PathSeg;
use std::fmt::Write;

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn bad(s: &str) -> bool {
    s.chars().any(char::is_control)
}

fn diagnostic(path: Vec<PathSeg>, what: &str) -> Diagnostic {
    Diagnostic {
        path,
        message: format!(
            "{what} cannot hold control characters: it is written as one line of kvrocks.conf"
        ),
        hint: None,
    }
}

/// Values that cannot be written into the config file safely, as diagnostics.
pub(crate) fn unwritable(spec: &KvrocksSpec) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if spec.password.as_deref().is_some_and(bad) {
        out.push(diagnostic(vec![key("password")], "a password"));
    }
    for (k, v) in &spec.settings {
        if bad(k) || k.contains(' ') || bad(v) {
            out.push(diagnostic(vec![key("settings"), key(k)], "a setting"));
        }
    }
    for (i, ns) in spec.namespace.iter().enumerate() {
        let at = |field: &str| {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            vec![key("namespace"), PathSeg::Index(i), key(field)]
        };
        if bad(&ns.name) || ns.name.contains(' ') {
            out.push(diagnostic(at("name"), "a namespace name"));
        }
        if bad(&ns.token) {
            out.push(diagnostic(at("token"), "a token"));
        }
    }
    out
}

/// The config file's text for `spec` listening on `backend`, and the check that the server is up.
pub(crate) fn render(
    data_dir: &str,
    backend: &Address,
    spec: &KvrocksSpec,
) -> Result<(String, Ready)> {
    let mut b = String::from("# Managed by doze — regenerated on every boot.\n");
    let _ = writeln!(b, "dir {data_dir}");
    let ready = match backend {
        // No TCP: an empty `bind` listens on no address; the port is kvrocks's own required placeholder.
        Address::Unix(socket) => {
            let _ = write!(b, "bind\nport 6666\nunixsocket {socket}\n");
            Ready::Socket(socket.clone())
        }
        Address::Tcp(addr) => {
            let (host, port) = help::host_port(addr)?;
            let _ = write!(b, "bind {host}\nport {port}\n");
            Ready::Tcp(addr.clone())
        }
    };
    if let Some(p) = spec.password() {
        let _ = writeln!(b, "requirepass {p}");
    }
    if let Some(w) = spec.workers.filter(|w| *w > 0) {
        let _ = writeln!(b, "workers {w}");
    }
    for (k, v) in help::settings(&spec.settings, &[]) {
        let _ = writeln!(b, "{k} {v}");
    }
    for ns in &spec.namespace {
        let _ = writeln!(b, "namespace.{} {}", ns.name, ns.token);
    }
    Ok((b, ready))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::Namespace;
    use std::collections::BTreeMap;

    fn spec(password: Option<&str>) -> KvrocksSpec {
        KvrocksSpec {
            workers: None,
            password: password.map(String::from),
            settings: BTreeMap::new(),
            namespace: vec![],
        }
    }

    // Go: TestWriteConf.
    #[test]
    fn a_socket_backend_listens_on_no_tcp_address() {
        let sock = Address::Unix("/run/store/kvrocks.sock".into());
        let (conf, ready) = render("/data/store", &sock, &spec(Some("pw"))).unwrap();
        for want in [
            "dir /data/store\n",
            "bind\n",
            "port 6666\n",
            "unixsocket /run/store/kvrocks.sock\n",
            "requirepass pw\n",
        ] {
            assert!(conf.contains(want), "conf lacks {want:?}:\n{conf}");
        }
        assert_eq!(ready, Ready::Socket("/run/store/kvrocks.sock".into()));
    }

    // Go: TestKvrocksNamespacesAndSettings.
    #[test]
    fn workers_settings_and_namespaces_are_lines_of_their_own() {
        let mut s = spec(Some("default-token"));
        s.workers = Some(4);
        s.settings
            .insert("rocksdb.block_size".into(), "16384".into());
        s.namespace.push(Namespace {
            name: "tenant_a".into(),
            token: "tok-a".into(),
        });
        let tcp = Address::Tcp("127.0.0.1:6380".into());
        let (conf, ready) = render("/d", &tcp, &s).unwrap();
        for want in [
            "bind 127.0.0.1\nport 6380\n",
            "workers 4\n",
            "rocksdb.block_size 16384\n",
            "namespace.tenant_a tok-a\n",
        ] {
            assert!(conf.contains(want), "conf lacks {want:?}:\n{conf}");
        }
        assert!(!conf.contains("unixsocket"));
        assert_eq!(ready, Ready::Tcp("127.0.0.1:6380".into()));
        assert!(unwritable(&s).is_empty());
    }

    #[test]
    fn a_value_that_would_start_another_line_is_refused() {
        let mut s = spec(Some("pw\nrequirepass other"));
        s.settings.insert("a".into(), "1\r".into());
        s.namespace.push(Namespace {
            name: "two words".into(),
            token: "t\n".into(),
        });
        let paths: Vec<_> = unwritable(&s).into_iter().map(|d| d.path).collect();
        assert_eq!(paths.len(), 4, "{paths:?}");
    }
}
