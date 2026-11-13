//! The addresses that work, as the machine's names say (Go `fronted`, `frontedDown`): while a doze process holds the
//! front door, an app's or service's http `.doze` address loses its port.

use super::Home;
use crate::machine_names;
use crate::view::StackView;

/// A running stack's app and service rows get the URL the registry gives their name (`url_for`): port-less while the
/// front door is up. Modules, tasks, and rows with an https domain keep theirs.
pub fn fronted(h: &mut Home, st: &StackView, mut url_for: impl FnMut(&str) -> Option<String>) {
    for r in h.groups.iter_mut().flat_map(|g| g.rows.iter_mut()) {
        let Some(v) = st.instances.iter().find(|v| v.name == r.name) else {
            continue;
        };
        if v.kind == "module" || v.kind == "task" || v.domain.is_empty() || !v.https.is_empty() {
            continue;
        }
        if let Some(url) = url_for(&v.domain) {
            r.address.clone_from(&url);
            r.url = url;
        }
    }
}

/// A stopped stack's web rows get the address they will have once up, when the front door is held on `front`.
pub fn fronted_down(h: &mut Home, front: Option<&str>) {
    let Some(front) = front else { return };
    for r in h.groups.iter_mut().flat_map(|g| g.rows.iter_mut()) {
        if let Some(a) = machine_names::fronted_down(&r.address, front) {
            r.address = a;
        }
    }
}
