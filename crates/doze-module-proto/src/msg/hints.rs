//! What protocol 1's core hard-coded per engine and a protocol-2 module declares in its manifest instead: the shell
//! client that opens a session on an instance, and how `doze init` recognises a project that uses the engine.

use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// A shell client command, as templates. In `args` and `env`, `{host}`, `{port}`, `{url}` and `{password}` become the
/// instance's; an `env` entry that comes out empty is left out (no password, no variable). A secret belongs in `env`
/// (the client's own variable for it), never in `args`, where `ps` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct ClientCommand {
    /// Looked for in the engine's own build first, then on `PATH`.
    #[n(0)]
    pub program: String,
    #[n(1)]
    #[cbor(default)]
    pub args: Vec<String>,
    #[n(2)]
    #[cbor(default)]
    pub env: BTreeMap<String, String>,
    /// Other programs taking the same arguments, tried on `PATH` in order when `program` is nowhere (kvrocks ships no
    /// client; `valkey-cli` and `redis-cli` speak to it).
    #[n(3)]
    #[cbor(default)]
    pub fallbacks: Vec<String>,
}

/// Signs that a project uses the engine.
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct Detect {
    /// By ecosystem ("go", "npm", "pypi", "cargo", …): package names, or Go import-path prefixes.
    #[n(0)]
    #[cbor(default)]
    pub packages: BTreeMap<String, Vec<String>>,
    /// Container image names, without a tag ("valkey/valkey").
    #[n(1)]
    #[cbor(default)]
    pub images: Vec<String>,
    /// Environment variables a project reads, and what each wants: "url" or "address".
    #[n(2)]
    #[cbor(default)]
    pub env: BTreeMap<String, String>,
}

impl ClientCommand {
    /// The arguments and environment for one instance. `vars` holds `host`, `port`, `url` and `password`; a value is
    /// put in as it is and never read again for placeholders (a password holding `{port}` stays a password), and an
    /// unknown `{name}` is left as written.
    pub fn render(&self, vars: &BTreeMap<&str, String>) -> (Vec<String>, BTreeMap<String, String>) {
        let args = self.args.iter().map(|a| fill(a, vars)).collect();
        let env = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), fill(v, vars)))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        (args, env)
    }
}

fn fill(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        let named = after.find('}').and_then(|close| {
            let name = after.get(..close)?;
            Some((vars.get(name)?, close))
        });
        match named {
            Some((value, close)) => {
                out.push_str(value);
                rest = after.get(close + 1..).unwrap_or_default();
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_template_fills_once_and_drops_empty_secrets() {
        let cmd = ClientCommand {
            program: "valkey-cli".into(),
            args: vec![
                "-h".into(),
                "{host}".into(),
                "-p".into(),
                "{port}".into(),
                "{nope}".into(),
            ],
            env: BTreeMap::from([("REDISCLI_AUTH".into(), "{password}".into())]),
            fallbacks: vec![],
        };
        let vars = |pw: &str| {
            BTreeMap::from([
                ("host", "127.0.0.1".to_string()),
                ("port", "6379".to_string()),
                ("password", pw.to_string()),
            ])
        };
        let (args, env) = cmd.render(&vars("p{port}w"));
        assert_eq!(args, ["-h", "127.0.0.1", "-p", "6379", "{nope}"]);
        assert_eq!(
            env.get("REDISCLI_AUTH").map(String::as_str),
            Some("p{port}w")
        );
        let (_, env) = cmd.render(&vars(""));
        assert!(env.is_empty(), "no password, no variable");
    }
}
