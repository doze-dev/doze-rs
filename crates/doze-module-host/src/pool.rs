//! One module process per engine, started on first use, started again after it dies, and stopped when idle (a Rust
//! module starts in milliseconds, so nothing needs to stay warm: protocol.md row 13).

use crate::running::{self, Running};
use crate::services::Services;
use doze_module_proto::msg::error::{Error, ErrorKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Finds the module program for an engine (the core: the module cache; tests: a built binary).
pub type Locate = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

/// The modules this host runs.
pub struct Pool {
    services: Services,
    locate: Locate,
    idle: Duration,
    modules: Mutex<HashMap<String, Arc<Running>>>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("idle", &self.idle)
            .finish_non_exhaustive()
    }
}

impl Pool {
    /// A pool that stops modules unused for `idle`.
    pub fn new(services: Services, locate: Locate, idle: Duration) -> Pool {
        Pool {
            services,
            locate,
            idle,
            modules: Mutex::new(HashMap::new()),
        }
    }

    /// The running module for `engine`, starting it (again) if needed.
    pub async fn get(&self, engine: &str) -> Result<Arc<Running>, Error> {
        let mut modules = self.modules.lock().await;
        if let Some(m) = modules.get(engine).filter(|m| m.is_alive()) {
            return Ok(m.clone());
        }
        let program = (self.locate)(engine)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("no module for {engine}")))?;
        let m = Arc::new(running::launch(engine, &program, self.services.clone()).await?);
        modules.insert(engine.to_string(), m.clone());
        Ok(m)
    }

    /// Stops modules idle for longer than the pool's limit; returns their engines. Call it now and then.
    pub async fn reap_idle(&self) -> Vec<String> {
        let idle: Vec<(String, Arc<Running>)> = {
            let mut modules = self.modules.lock().await;
            let names: Vec<String> = modules
                .iter()
                .filter(|(_, m)| !m.is_alive() || m.idle_for() > self.idle)
                .map(|(k, _)| k.clone())
                .collect();
            names
                .into_iter()
                .filter_map(|k| modules.remove(&k).map(|m| (k, m)))
                .collect()
        };
        for (_, m) in &idle {
            m.shutdown(Duration::from_secs(2)).await;
        }
        idle.into_iter().map(|(k, _)| k).collect()
    }

    /// Engines with a module process right now.
    pub async fn running(&self) -> Vec<String> {
        self.modules
            .lock()
            .await
            .iter()
            .filter(|(_, m)| m.is_alive())
            .map(|(k, _)| k.clone())
            .collect()
    }
}
