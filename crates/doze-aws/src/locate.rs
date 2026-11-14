//! Finding the running doze-aws a command means: the one holding the configured data directory (its `instance.json`), else
//! whatever answers `/_doze/health` at the configured address (a server started from another directory, or in a container).

use std::time::Duration;

use crate::client::Client;
use crate::config::Config;
use crate::instance::{self, Instance, endpoint_of};

/// The running doze-aws for this configuration, if there is one.
pub(crate) fn find(cfg: &Config) -> Option<Instance> {
    instance::running(&cfg.data_dir()).or_else(|| by_health(cfg))
}

fn by_health(cfg: &Config) -> Option<Instance> {
    let listen: std::net::SocketAddr = cfg.text("listen").parse().ok()?;
    if listen.port() == 0 {
        return None;
    }
    let found = health(&endpoint_of(listen))?;
    // The health answer leaves the keys out: under verify they are the configured ones.
    let verify = found.auth == "verify";
    Some(Instance {
        access_key: verify.then(|| cfg.text("access-key")),
        secret_key: verify.then(|| cfg.text("secret-key")),
        ..found
    })
}

/// What a doze-aws at `endpoint` says about itself, if one answers there.
pub(crate) fn health(endpoint: &str) -> Option<Instance> {
    let client = Client::new(endpoint, "us-east-1", "test", "test").ok()?;
    let reply = client
        .get("/_doze/health", Duration::from_millis(800))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&reply.body).ok()?;
    (reply.status == 200 && value.get("name").and_then(|n| n.as_str()) == Some("doze-aws"))
        .then(|| serde_json::from_value::<Instance>(value).ok())
        .flatten()
        .map(|i| Instance {
            endpoint: endpoint.to_owned(),
            ..i
        })
}

impl Instance {
    /// The keys a client of this instance signs with.
    pub(crate) fn keys(&self) -> (String, String) {
        match (&self.access_key, &self.secret_key) {
            (Some(a), Some(s)) if self.auth == "verify" => (a.clone(), s.clone()),
            _ => ("test".to_owned(), "test".to_owned()),
        }
    }

    /// A client of this instance, in its region.
    pub(crate) fn client(&self) -> Result<Client, String> {
        let (access, secret) = self.keys();
        Client::new(&self.endpoint, &self.region, &access, &secret)
    }
}

/// What to tell someone when nothing is running.
pub(crate) fn not_running(cfg: &Config) -> String {
    format!(
        "doze-aws is not running: nothing holds {} and nothing answers at {}.\n  Start it:  doze-aws",
        crate::config::display_path(
            &cfg.data_dir(),
            &std::env::current_dir().unwrap_or_default()
        ),
        cfg.text("listen"),
    )
}
