//! Whether the system's resolver agrees with the zone before a stack's apps start. A name the zone serves can still be
//! "no such name" to the system for a while: macOS keeps a negative answer it got while the stack was down (an older
//! network daemon sent it with no expiry hint, so the system picked its own), and an app that looks the name up at
//! boot then crashes with ENOTFOUND. The zone answers at once; the system's cache is what lags. So a stack coming up
//! asks the system the way an app would (getaddrinfo), says which names it still refuses and why, and waits a bounded
//! time for them, rather than letting the apps find out.
//!
//! Only where the system's answers come from this registry: a live program serving it holds the resolver and the
//! system routes the zone there. Elsewhere (a test's home, a machine not set up) "no such name" is not a stale cache.

use crate::{Registry, SUFFIX};
use std::time::{Duration, Instant};

/// How long a stack waits for the system to stop refusing its names.
pub const BUDGET: Duration = Duration::from_secs(20);
/// How often the system is asked again meanwhile.
pub const EVERY: Duration = Duration::from_millis(250);

/// Whether the system resolves `host` now: its caches, its routes, then the zone's resolver, as an app's lookup goes.
pub fn system_resolves(host: &str) -> bool {
    use std::net::ToSocketAddrs;
    (host, 0u16)
        .to_socket_addrs()
        .is_ok_and(|mut a| a.next().is_some())
}

/// Whether the system asks this registry's zone: a live program serving the registry holds the resolver, and the
/// system routes the zone to it.
pub fn system_asks_here(reg: &Registry) -> bool {
    reg.resolver_holder()
        .is_some_and(|h| u32::try_from(h.pid).is_ok_and(doze_os::process::alive))
        && crate::trust_store::domain_routed(SUFFIX).0
}

/// Waits, up to `budget`, until `resolves` says yes for every host, asking every `every`. Says once which names the
/// system still refuses, and once how it ended. Returns the hosts it never resolved.
pub fn wait_resolved(
    hosts: &[String],
    (budget, every): (Duration, Duration),
    resolves: &dyn Fn(&str) -> bool,
    log: &dyn Fn(String),
) -> Vec<String> {
    let start = Instant::now();
    let mut left: Vec<String> = hosts.iter().filter(|h| !resolves(h)).cloned().collect();
    if left.is_empty() {
        return left;
    }
    log(format!(
        "names: the system still says {} {} not exist: an answer it cached while the stack was down; waiting up to \
         {}s for it to clear before starting apps{}",
        left.join(", "),
        if left.len() == 1 { "does" } else { "do" },
        budget.as_secs(),
        FLUSH_HINT
    ));
    while !left.is_empty() && start.elapsed() < budget {
        std::thread::sleep(every);
        left.retain(|h| !resolves(h));
    }
    if left.is_empty() {
        log(format!(
            "names: the system resolves them now (after {:.1}s)",
            start.elapsed().as_secs_f32()
        ));
    } else {
        log(format!(
            "names: the system still refuses {} after {}s; apps that look them up may fail to connect{}",
            left.join(", "),
            budget.as_secs(),
            FLUSH_HINT
        ));
    }
    left
}

/// How a person clears the system's cache at once, where there is a way.
const FLUSH_HINT: &str = if cfg!(target_os = "macos") {
    " (sudo killall -HUP mDNSResponder clears it at once)"
} else {
    ""
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn says_nothing_and_waits_for_nothing_when_every_name_resolves() {
        let said = Mutex::new(Vec::new());
        let left = wait_resolved(
            &["api.shop.doze".into()],
            (Duration::from_secs(5), Duration::from_millis(1)),
            &|_| true,
            &|l| said.lock().unwrap().push(l),
        );
        assert!(left.is_empty());
        assert!(said.lock().unwrap().is_empty());
    }

    #[test]
    fn a_stale_negative_answer_is_named_and_waited_out() {
        let asked = AtomicUsize::new(0);
        let said = Mutex::new(Vec::new());
        // The system refuses db.shop.doze for its first three asks, then answers.
        let resolves = |h: &str| h != "db.shop.doze" || asked.fetch_add(1, Ordering::SeqCst) >= 3;
        let left = wait_resolved(
            &["api.shop.doze".into(), "db.shop.doze".into()],
            (Duration::from_secs(5), Duration::from_millis(1)),
            &resolves,
            &|l| said.lock().unwrap().push(l),
        );
        assert!(left.is_empty());
        let said = said.lock().unwrap();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said[0].contains("db.shop.doze does not exist") && !said[0].contains("api.shop.doze"),
            "{said:?}"
        );
        assert!(said[1].contains("resolves them now"), "{said:?}");
    }

    #[test]
    fn gives_up_after_the_budget_and_says_so() {
        let said = Mutex::new(Vec::new());
        let left = wait_resolved(
            &["db.shop.doze".into()],
            (Duration::from_millis(20), Duration::from_millis(5)),
            &|_| false,
            &|l| said.lock().unwrap().push(l),
        );
        assert_eq!(left, vec!["db.shop.doze".to_string()]);
        assert!(said.lock().unwrap()[1].contains("still refuses db.shop.doze"));
    }

    #[test]
    fn a_registry_nobody_serves_is_not_asked_by_the_system() {
        let home = std::env::temp_dir().join(format!("dz-settle-{}", std::process::id()));
        let reg = Registry::open(&home, "test");
        assert!(!system_asks_here(&reg));
        let _ = std::fs::remove_dir_all(&home);
    }
}
