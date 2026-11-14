//! What a module or app publishes to the blocks that reference it (`module.db.url`), and its
//! address (Go's `attributesFor` and `InstanceAddr`).

use crate::model::{COMMAND_ENGINE, Config, Instance, ModuleBody, Spec};
use crate::modules::{Call, Endpoint, Modules};
use doze_hcl::Value;
use std::collections::BTreeMap;

/// Go's `net.SplitHostPort` (brackets around an IPv6 host removed).
pub fn split_host_port(addr: &str) -> Option<(String, String)> {
    if let Some(rest) = addr.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':')?;
        return (!port.contains(':')).then(|| (host.to_owned(), port.to_owned()));
    }
    let (host, port) = addr.rsplit_once(':')?;
    if host.contains(':') {
        return None;
    }
    Some((host.to_owned(), port.to_owned()))
}

/// Go's `net.JoinHostPort`.
pub fn join_host_port(host: &str, port: &str) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Go's `InstanceAddr`: where it listens; "" for a command with no port (nothing to collide);
/// an error for a module that needs a port and has none.
pub fn instance_addr(inst: &Instance, modules: &dyn Modules) -> Result<String, String> {
    if !inst.listen.is_empty() {
        return Ok(inst.listen.clone());
    }
    if inst.port != 0 {
        return Ok(join_host_port("127.0.0.1", &inst.port.to_string()));
    }
    let supervised = inst.ty == COMMAND_ENGINE
        || modules
            .cached(&inst.ty)
            .or_else(|| modules.lookup(&inst.ty, &inst.version))
            .is_some_and(|m| m.supervised);
    if supervised {
        return Ok(String::new());
    }
    Err(format!(
        "{} has no port — add `port = NNNN` to the block (e.g. port = 5432); doze does not auto-assign ports",
        inst.title()
    ))
}

fn s(v: &str) -> Value {
    Value::string(v)
}

/// `socket`, `host` and `port` from the endpoint.
fn where_attrs(m: &mut BTreeMap<String, Value>, ep: &Endpoint) {
    match ep {
        Endpoint::Unix(path) => {
            m.insert("socket".into(), s(path));
            m.insert("host".into(), s(""));
            m.insert("port".into(), Value::int(0));
        }
        Endpoint::Tcp(a) => {
            if let Some((host, port)) = split_host_port(a) {
                m.insert("port".into(), Value::int(port.parse().unwrap_or(0)));
                m.insert("host".into(), s(&host));
                m.insert("socket".into(), s(""));
            }
        }
    }
}

/// Go's `attributesFor`.
pub(crate) fn attributes_for(cfg: &Config, modules: &dyn Modules, idx: usize) -> Value {
    let Some(inst) = cfg.instances.get(idx) else {
        return Value::Object(BTreeMap::new());
    };
    let mut m = BTreeMap::from([
        ("name".to_owned(), s(&inst.name)),
        ("engine".to_owned(), s(&inst.ty)),
        ("env".to_owned(), Value::Object(BTreeMap::new())),
    ]);
    let mut addr = match instance_addr(inst, modules) {
        Ok(a) if !a.is_empty() => a,
        _ => return Value::Object(m),
    };
    if cfg.defaults.names
        && let Some((host, port)) = split_host_port(&addr)
        && !host.is_empty()
    {
        addr = join_host_port(&cfg.domain_for(&inst.name), &port);
    }
    let ep = match addr.strip_prefix("unix:") {
        Some(p) => Endpoint::Unix(p.to_owned()),
        None => Endpoint::Tcp(addr.clone()),
    };
    let empty = ModuleBody::default();
    let body = match &inst.spec {
        Spec::Module(b) => b,
        _ => &empty,
    };
    let call = Call {
        ty: &inst.ty,
        name: &inst.name,
        version: &inst.version,
        endpoint: &ep,
        body,
    };
    let published = if inst.ty == COMMAND_ENGINE {
        Default::default()
    } else {
        modules.attributes(&call)
    };
    m.insert("address".into(), s(&addr));
    m.insert("url".into(), s(&published.url));
    m.insert("env_var".into(), s(&published.env_var));
    if !published.env_var.is_empty() {
        m.insert(
            "env".into(),
            Value::Object(BTreeMap::from([(
                published.env_var.clone(),
                s(&published.url),
            )])),
        );
    }
    where_attrs(&mut m, &ep);
    if inst.ty == COMMAND_ENGINE {
        if let Endpoint::Tcp(a) = &ep
            && let Some((host, port)) = split_host_port(a)
        {
            m.insert("url".into(), s(&format!("http://{host}:{port}")));
        }
    } else {
        m.extend(published.extras);
    }
    Value::Object(m)
}
