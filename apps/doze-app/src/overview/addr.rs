//! Addresses on the stack's page: the copy mark and `y` (Go `copy`: the URL, else the address, and a toast that says
//! what went to the clipboard).

use gpui::Context;

use super::{OverviewView, all_rows};

impl OverviewView {
    /// Puts `text` on the clipboard and says so: "copied the address of api to the clipboard".
    pub(super) fn copy_text(&mut self, text: &str, what: &str, cx: &mut Context<Self>) {
        crate::outward::copy(cx, text);
        self.say(&format!("copied {what} to the clipboard"), cx);
    }

    /// `y`: the selected row's URL, else its address.
    pub(super) fn copy_address(&mut self, cx: &mut Context<Self>) {
        let row = self.selected.as_ref().and_then(|s| {
            all_rows(&self.home)
                .into_iter()
                .find(|r| &r.name == s)
                .cloned()
        });
        match row {
            Some(r) if !r.address.is_empty() => {
                let a = if r.url.is_empty() { &r.address } else { &r.url };
                self.copy_text(a, &format!("the address of {}", r.name), cx);
            }
            Some(r) => self.say(&format!("{} has no address to copy.", r.name), cx),
            None => {}
        }
    }
}

impl OverviewView {
    /// What the machine's names change on the page: addresses port-less behind the front door (Go `fronted`,
    /// `frontedDown`), and the setup banner while the stack's own domains are not set up here (Go `setupFor`).
    pub(super) fn machine(
        &mut self,
        h: &mut doze_desk::overview::Home,
        st: &doze_desk::StackView,
        cx: &mut Context<Self>,
    ) {
        let home = self.hub.home().to_path_buf();
        self.facts.update(cx, |f, cx| {
            if st.running {
                doze_desk::overview::fronted(h, st, |host| f.url_for(&home, host));
                // The file is read again each time it is saved (Go `configLook`): a mistake in it is the red banner,
                // and takes the place of "changed".
                let err = f.shape(&st.dir, cx).and_then(|s| s.error);
                if f.checking(&st.dir) {
                    // Saved a moment ago: whether it loads is known in a moment; "changed" would be a guess.
                    h.changed = None;
                }
                if let Some(err) = err {
                    let file = std::path::Path::new(&st.dir).join("doze.hcl");
                    let saved = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
                    let now = self.hub.now();
                    h.config = Some(doze_desk::overview::config_error(&file, &err, saved, now));
                    h.changed = None;
                }
            } else {
                let front = f.front_door(&home);
                doze_desk::overview::fronted_down(h, front.as_deref());
            }
            h.setup = f.setup(st, &home, cx);
            if let Some(su) = h.setup.as_mut() {
                su.prompt = f.live() && doze_os::privilege::admin_supported();
            }
        });
    }
}
