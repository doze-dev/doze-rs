//! What the core grants an instance before it plans: for each listener a loopback address and a backend for the
//! engine, a socket file on Unix and a loopback port on Windows (PLAN.md D13).

use crate::sandbox::{Sandbox, free_port};
use doze_module_proto::msg::instance::{Address, Granted, Instance, Listener};
use doze_module_proto::network;
use std::collections::BTreeMap;

/// Grants every listener a loopback address and a backend: a socket file on Unix, a loopback port on Windows (D13).
pub(crate) fn grant(
    listeners: &[Listener],
    sandbox: &Sandbox,
    inst: &Instance,
) -> Result<BTreeMap<String, Granted>, String> {
    let mut out = BTreeMap::new();
    let socket_dir = sandbox.socket_dir(&inst.name).map_err(|e| e.to_string())?;
    for l in listeners {
        let port = free_port().map_err(|e| e.to_string())?;
        let backend = if cfg!(unix) {
            Address::Unix(
                socket_dir
                    .join(format!("{}.sock", l.name))
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            Address::Tcp(format!(
                "127.0.0.1:{}",
                free_port().map_err(|e| e.to_string())?
            ))
        };
        let hosts: Vec<String> = l
            .routes
            .iter()
            .filter_map(|r| network::route_host(r, &inst.host).ok())
            .collect();
        let scheme = if l.http { "http" } else { "tcp" };
        out.insert(
            l.name.clone(),
            Granted {
                addr: format!("127.0.0.1:{port}"),
                url: format!("{scheme}://127.0.0.1:{port}"),
                hosts,
                backend,
            },
        );
    }
    Ok(out)
}
