//! The `.doze` zone's machine setup, part by part (Go doctor's `names: …` lines), and which program answers for the
//! zone. The setup is shared with doze-aws and doze-kafka: whichever of them set the machine up serves all three.

use crate::{Check, FIX_NAMES, Machine, State};
use doze_names::Registry;

/// The zone's checks: each part of the setup, then who holds the resolver and the front door.
pub fn checks(m: &Machine) -> Vec<Check> {
    if m.zone_off {
        return vec![Check::new(
            "names",
            State::Ok,
            format!(
                "the .doze zone is switched off ({}=off): nothing to set up",
                doze_names::ENV_ZONE
            ),
        )];
    }
    let mut out: Vec<Check> = m
        .setup
        .check()
        .steps
        .into_iter()
        .map(|s| Check::step(format!("names: {}", s.name), s.done, s.detail).fix(FIX_NAMES))
        .collect();
    out.extend(holders(m));
    out
}

/// Whether every part of the zone's setup is in place (nothing to repair).
pub fn set_up(m: &Machine) -> bool {
    m.zone_off || m.setup.check().ok()
}

/// Who answers for the zone and who holds the front door: whichever doze program started first serves all, so it need
/// not be this stack's daemon. A holder whose process is gone is a leftover, said by [`crate::leftovers`].
fn holders(m: &Machine) -> Vec<Check> {
    let zone = Registry::open(m.home(), "doze doctor");
    let live = |e: &doze_names::Entry| u32::try_from(e.pid).is_ok_and(doze_os::process::alive);
    let mut out = Vec::new();
    if let Some(h) = zone.resolver_holder().filter(live) {
        out.push(Check::new(
            "names: resolver",
            State::Ok,
            format!(
                "{} (pid {}) is answering .{}",
                h.owner,
                h.pid,
                doze_names::SUFFIX
            ),
        ));
    }
    if let Some(h) = zone.ingress_holder().filter(live) {
        out.push(Check::new(
            "names: front door",
            State::Ok,
            format!(
                "{} (pid {}) is serving port-less http:// names",
                h.owner, h.pid
            ),
        ));
    }
    out
}
