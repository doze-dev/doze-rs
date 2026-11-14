//! The engine's background work: the idle reaper (Go's `RunReaper`) and the health prober of supervised commands
//! (Go's `RunHealthProber`, which records and never restarts: a blip must not cause a flap).

use crate::engine::Engine;
use crate::lifecycle::Now;
use doze_module_proto::msg::run::Ready;
use std::time::Duration;
use tokio::task::JoinHandle;

/// How often the health prober looks.
pub const HEALTH_EVERY: Duration = Duration::from_secs(3);

/// The background tasks of an engine; dropping this stops them (not the instances).
#[derive(Debug)]
pub struct Background {
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Background {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

/// The reaper's tick: a tenth of the idle time, within 250 ms to 1 s, so an instance sleeps within about a second of
/// its deadline and the "sleeps in" countdown stays honest.
pub fn reap_interval(idle: Option<Duration>) -> Duration {
    let i = idle.map_or(Duration::ZERO, |d| d / 10);
    if i.is_zero() || i > Duration::from_secs(1) {
        Duration::from_secs(1)
    } else {
        i.max(Duration::from_millis(250))
    }
}

impl Engine {
    /// Starts the reaper and the health prober.
    pub fn background(&self) -> Background {
        let reaper = tokio::spawn(reap_loop(self.clone()));
        let health = tokio::spawn(health_loop(self.clone()));
        Background {
            tasks: vec![reaper, health],
        }
    }

    /// One pass of the reaper: stops what has been idle for its time (decided under the lock, so a client that just
    /// connected is never cut off). Returns what it stopped.
    pub async fn reap_once(&self) -> Vec<String> {
        let fx = self.update(|c| c.m.reap(Now::current()));
        let slept: Vec<String> = fx
            .iter()
            .filter_map(|f| match f {
                crate::machine::Effect::StopUnit { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for w in self.run(fx) {
            let _ = w.await;
        }
        for name in &slept {
            tracing::info!("\"{name}\" went to sleep: nothing held it for its idle time");
        }
        slept
    }
}

async fn reap_loop(e: Engine) {
    let mut tick = tokio::time::interval(reap_interval(e.stack().idle_timeout));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        e.reap_once().await;
    }
}

async fn health_loop(e: Engine) {
    let mut tick = tokio::time::interval(HEALTH_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        probe_once(&e).await;
    }
}

/// One health probe's limit (Go's).
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Probes every running supervised command. With no check, or a one-shot `log_line` one, a running command counts as
/// healthy (Go's `CheckHealth` returned nil for those).
pub(crate) async fn probe_once(e: &Engine) {
    let checks: Vec<(String, Option<Ready>)> = e.update(|c| {
        c.live
            .iter()
            .filter(|(n, l)| l.supervised && c.m.table().state(n).is_some_and(|s| s.running()))
            .map(|(n, l)| {
                (
                    n.clone(),
                    l.health.clone().filter(|r| !matches!(r, Ready::LogLine(_))),
                )
            })
            .collect()
    });
    for (name, ready) in checks {
        let healthy = match ready {
            None => true,
            Some(r) => matches!(
                tokio::time::timeout(
                    HEALTH_TIMEOUT,
                    crate::ready::probe(&r, std::path::Path::new(""))
                )
                .await,
                Ok(Ok(true))
            ),
        };
        e.update(|c| {
            if c.m.table().state(&name).is_some_and(|s| s.running()) {
                c.m.table_mut().set_healthy(&name, healthy);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_follows_gos_rule() {
        let ms = Duration::from_millis;
        assert_eq!(reap_interval(None), ms(1000));
        assert_eq!(reap_interval(Some(Duration::from_secs(300))), ms(1000));
        assert_eq!(reap_interval(Some(ms(5000))), ms(500));
        assert_eq!(reap_interval(Some(ms(1000))), ms(250));
    }
}
