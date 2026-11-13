//! The machine's names as the stack page needs them (Go `setupFor`, `urlFor`, `frontDoorPort`): whether a stack's
//! domains are set up here, asked off the UI thread and kept a minute (the checks read files and the keychain); and
//! who holds the front door, kept two seconds (a file read). Read-only: the setup itself is `doze trust`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use doze_desk::StackView;
use doze_desk::machine_names::{self as names, Setup, Step};
use gpui::{AsyncApp, Context, WeakEntity};

use super::Facts;

/// How long a stack's domain setup is kept (Go: a minute).
const TRUST_AGE: Duration = Duration::from_secs(60);
/// How long the front door's holder is kept (Go `configLook`).
const FRONT_AGE: Duration = Duration::from_secs(2);

impl Facts {
    /// The setup banner for `st`: `None` while its domains are all set up (or it declares none), and while the first
    /// answer is coming.
    pub(crate) fn setup(
        &mut self,
        st: &StackView,
        home: &Path,
        cx: &mut Context<Self>,
    ) -> Option<Setup> {
        let domains = names::domains(st);
        if domains.is_empty() {
            return None;
        }
        let key = format!("{}|{}", st.key.dir().display(), domains.join(","));
        let fresh = self
            .trust
            .get(&key)
            .is_some_and(|(at, _)| at.elapsed() < TRUST_AGE);
        if !fresh && self.trust_asked.insert(key.clone()) {
            let (d, h, k) = (domains.clone(), PathBuf::from(home), key.clone());
            let job = cx
                .background_executor()
                .spawn(async move { names::trust_status(&d, &h) });
            cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
                let steps: Vec<Step> = job.await;
                let _ = this.update(cx, |f, cx| {
                    f.trust_asked.remove(&k);
                    let changed = f.trust.get(&k).is_none_or(|(_, s)| *s != steps);
                    f.trust.insert(k, (Instant::now(), steps));
                    if changed {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let (_, steps) = self.trust.get(&key)?;
        names::setup_of(&domains, steps)
    }

    /// The port a doze process holds the front door on, if one does.
    pub(crate) fn front_door(&mut self, home: &Path) -> Option<String> {
        if let Some((at, port)) = &self.front
            && at.elapsed() < FRONT_AGE
        {
            return port.clone();
        }
        let port = names::front_door_port(home);
        self.front = Some((Instant::now(), port.clone()));
        port
    }
}

impl Facts {
    /// The CLI is reading `dir`'s doze.hcl again (it was saved): whether it loads is not known yet.
    pub(crate) fn checking(&self, dir: &str) -> bool {
        self.asking.contains(Path::new(dir))
    }

    /// The registry's URL for a `.doze` host (Go `urlFor`), kept two seconds a host.
    pub(crate) fn url_for(&mut self, home: &Path, host: &str) -> Option<String> {
        if let Some((at, url)) = self.urls.get(host)
            && at.elapsed() < FRONT_AGE
        {
            return url.clone();
        }
        let url = names::url_for(home, host);
        self.urls
            .insert(host.to_owned(), (Instant::now(), url.clone()));
        url
    }
}
