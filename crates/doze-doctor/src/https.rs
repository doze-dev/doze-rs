//! https:// for the `.doze` names: the front door answers every name it fronts over https with a leaf from the
//! machine's authority (`<home>/ca`), so a client believes it only once this machine trusts that authority. No declared
//! domain is needed. Until then every `https://<name>.doze` fails, and nothing at the client says why, so doctor does.
//!
//! The repair is part of the zone's own (`doze doctor --fix`, `doze dns-setup`): one password sets up the network
//! daemon, the resolver route and this. The authority is files under the doze home (no privilege), made just before the
//! script runs, so a dry run shows the real script and makes nothing.

use crate::{Check, FIX_HTTPS, FIX_ROTATE, Machine, State};
use doze_names::ca::{Ca, ca_dir};
use doze_names::setup::{Os, Plan};
use doze_names::trust_store::ca_trusted;

/// The check's name: under `names: `, which the app's Settings › Machine groups by.
pub const NAME: &str = "names: https";

/// Where the authority stands for https.
#[derive(Debug)]
pub enum Https {
    /// Trusted here: says where.
    Trusted(String),
    /// Made, and not trusted here.
    Untrusted(Box<Ca>),
    /// Not made yet.
    Missing,
    /// There and unusable (`doze trust --rotate` replaces it).
    Broken(String),
}

impl Https {
    /// Looks, without privilege and without making anything.
    pub fn probe(m: &Machine) -> Https {
        match Ca::probe(&ca_dir(m.home())) {
            Err(e) => Https::Broken(e.to_string()),
            Ok(None) => Https::Missing,
            Ok(Some(ca)) => match ca_trusted(&ca) {
                (true, where_) => Https::Trusted(where_),
                (false, _) => Https::Untrusted(Box::new(ca)),
            },
        }
    }
}

/// Whether https for the zone is something this system can set up.
fn applies(m: &Machine) -> bool {
    !m.zone_off && matches!(m.setup.os, Os::Mac | Os::Linux | Os::Windows)
}

/// Doctor's line for https, or none where it does not apply.
pub fn check(m: &Machine) -> Option<Check> {
    if !applies(m) {
        return None;
    }
    Some(match Https::probe(m) {
        Https::Trusted(where_) => Check::new(NAME, State::Ok, format!("https:// names are trusted ({where_})")),
        Https::Untrusted(ca) => Check::new(
            NAME,
            State::Fail,
            format!(
                "{} is not trusted here, so clients refuse https:// names — run {FIX_HTTPS} (one password)",
                ca.common_name()
            ),
        )
        .fix(FIX_HTTPS),
        Https::Missing => Check::new(
            NAME,
            State::Fail,
            format!(
                "no certificate authority yet: https:// names are refused until {FIX_HTTPS} (one password) makes \
                 and trusts one"
            ),
        )
        .fix(FIX_HTTPS),
        Https::Broken(why) => Check::new(
            NAME,
            State::Fail,
            format!("the certificate authority cannot be used ({why}), so https:// names are refused"),
        )
        .fix(FIX_ROTATE),
    })
}

/// The repair's part for https: the script, and whether the authority must be made before it runs. `None` when there
/// is nothing to do or it cannot be done here (a broken authority is said by the domains' part and by the check).
pub fn plan(m: &Machine) -> Option<(Plan, bool)> {
    if !applies(m) {
        return None;
    }
    let os = m.setup.os;
    match Https::probe(m) {
        Https::Trusted(_) | Https::Broken(_) => None,
        Https::Untrusted(ca) => {
            doze_names::setup::trust::https_plan(os, m.home(), Some(&ca)).map(|p| (p, false))
        }
        Https::Missing => {
            doze_names::setup::trust::https_plan(os, m.home(), None).map(|p| (p, true))
        }
    }
}

/// Whether https for the zone needs nothing done.
pub fn set_up(m: &Machine) -> bool {
    check(m).is_none_or(|c| c.state != State::Fail)
}
