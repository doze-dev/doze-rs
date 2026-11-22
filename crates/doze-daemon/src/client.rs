//! The client command `doze connect` runs (additive to Go's protocol 2): the module's declared client
//! (`ClientCommand`) rendered for one instance, with doze's address for it (so connecting wakes it) and the password its
//! connection URL carries, and the program's place in the engine's own build once a start has resolved it.

use crate::Daemon;
use doze_control::{ClientView, InstanceView};
use doze_module_proto::msg::hints::ClientCommand;
use doze_module_proto::msg::value::Value;
use std::collections::BTreeMap;

/// `cmd` for an instance reached at `endpoint` (`host:port`) whose connection URL is `url`, and whose module publishes
/// `extras`: each text attribute is a placeholder too (aws: `{region}`); doze's own four win a name.
pub(crate) fn view(
    cmd: &ClientCommand,
    endpoint: &str,
    url: &str,
    extras: &BTreeMap<String, Value>,
) -> ClientView {
    let (host, port) = endpoint.rsplit_once(':').unwrap_or((endpoint, ""));
    let mut vars: BTreeMap<&str, String> = extras
        .iter()
        .filter_map(|(k, v)| match v {
            Value::String(s) => Some((k.as_str(), s.clone())),
            _ => None,
        })
        .collect();
    vars.extend([
        (
            "host",
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
        ),
        ("port", port.to_string()),
        ("url", url.to_string()),
        ("password", password(url)),
    ]);
    let (args, env) = cmd.render(&vars);
    ClientView {
        program: cmd.program.clone(),
        path: String::new(),
        args,
        env,
        fallbacks: cmd.fallbacks.clone(),
    }
}

/// The password in a URL's user part (`scheme://user:pw@host`), as written; empty when there is none.
pub(crate) fn password(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority
        .rsplit_once('@')
        .and_then(|(user, _)| user.split_once(':'))
        .map(|(_, pw)| pw.to_string())
        .unwrap_or_default()
}

/// Sets each client's `path` to the program in its engine's build, when a start resolved one and the program is there.
pub(crate) fn fill_paths(d: &Daemon, views: &mut [InstanceView]) {
    for v in views {
        let Some(c) = v.connect.as_mut() else {
            continue;
        };
        let Some(tc) = d.module(&v.name).and_then(|m| m.toolchain()) else {
            continue;
        };
        let dir = std::path::Path::new(&tc.bin_dir);
        let found = [c.program.clone(), format!("{}.exe", c.program)]
            .into_iter()
            .map(|p| dir.join(p))
            .find(|p| p.is_file());
        if let Some(p) = found {
            c.path = p.to_string_lossy().into_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_is_rendered_for_the_instance() {
        let cmd = ClientCommand {
            program: "valkey-cli".into(),
            args: ["-h", "{host}", "-p", "{port}"].map(String::from).to_vec(),
            env: BTreeMap::from([("REDISCLI_AUTH".into(), "{password}".into())]),
            fallbacks: vec!["redis-cli".into()],
        };
        let v = view(
            &cmd,
            "127.0.0.1:6380",
            "redis://:s3cret@127.0.0.1:6380/0",
            &BTreeMap::new(),
        );
        assert_eq!(v.args, ["-h", "127.0.0.1", "-p", "6380"]);
        assert_eq!(v.env["REDISCLI_AUTH"], "s3cret");
        assert_eq!(v.fallbacks, ["redis-cli"]);
        let none = view(
            &cmd,
            "127.0.0.1:6380",
            "redis://127.0.0.1:6380",
            &BTreeMap::new(),
        );
        assert!(none.env.is_empty(), "an empty password is not set");
        let cmd = ClientCommand {
            program: "aws".into(),
            args: vec![],
            env: BTreeMap::from([("AWS_DEFAULT_REGION".into(), "{region}".into())]),
            fallbacks: vec![],
        };
        let extras = BTreeMap::from([("region".into(), Value::String("eu-west-2".into()))]);
        let v = view(&cmd, "127.0.0.1:4566", "http://127.0.0.1:4566", &extras);
        assert_eq!(v.env["AWS_DEFAULT_REGION"], "eu-west-2");
        assert_eq!(password("postgres://app:pw@h:5432/db?x=a@b"), "pw");
        assert_eq!(password("postgres://h:5432/db"), "");
    }
}
