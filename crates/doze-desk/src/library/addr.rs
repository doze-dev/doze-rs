//! A Library row's address pills (Go `addressesOf`, and `serve/library.go`'s pass over them): every https domain, then
//! the apps by name, three at most; an https one not set up here is no link; an app's or a service's goes port-less
//! behind the front door.

use std::collections::HashSet;

use doze_display as words;
use serde::Serialize;

use super::Library;
use crate::view::StackView;

/// One address pill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Addr {
    pub text: String,
    /// What Open uses; empty for a name that is not http.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    pub up: bool,
    /// An https domain this machine is not set up for yet: not a link, not up (Go `Unready`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unready: bool,
    /// An app's or a service's web name (Go `Addr.Host`): its pill is the registry's URL for it when there is one.
    #[serde(skip)]
    pub host: String,
}

impl Addr {
    /// Its title (Go `AddrPill`): why an unready one is no link, else what a click opens, else the name.
    #[must_use]
    pub fn title(&self) -> String {
        if self.unready {
            format!(
                "{} needs a one-time setup on this machine; the stack's page has it",
                self.text
            )
        } else if !self.url.is_empty() {
            format!("Open {}", self.url)
        } else {
            self.text.clone()
        }
    }
}

/// The https pills of the stacks keyed in `unset` (whose domains are not set up here) are not offered as links: the
/// stack's page says what the setup is. (Go `Library`.)
pub fn unready(lib: &mut Library, unset: &[String]) {
    for st in lib.stacks.iter_mut().filter(|s| unset.contains(&s.key)) {
        for a in st
            .addresses
            .iter_mut()
            .filter(|a| a.text.starts_with("https://"))
        {
            a.unready = true;
            a.url.clear();
            a.up = false;
        }
    }
}

/// An app's or a service's pill as the registry says it (`url_for` of its host: port-less while a front door holds
/// the port, Go `urlFor`), unless it is unready. (Go `Library`.)
pub fn fronted(lib: &mut Library, mut url_for: impl FnMut(&str) -> Option<String>) {
    for a in lib.stacks.iter_mut().flat_map(|s| s.addresses.iter_mut()) {
        if a.host.is_empty() || a.unready {
            continue;
        }
        if let Some(url) = url_for(&a.host).filter(|u| !u.is_empty()) {
            a.text.clone_from(&url);
            a.url = url;
        }
    }
}

/// The pills: every https domain, then the apps by name; three at most, and how many more there are.
pub(super) fn addresses(st: &StackView) -> (Vec<Addr>, usize) {
    let snap = st.words();
    let pairs: Vec<_> = st.instances.iter().zip(&snap.instances).collect();
    let mut out: Vec<Addr> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for (v, i) in &pairs {
        for d in v.domains.iter().filter(|d| !d.starts_with("*.")) {
            if seen.insert(d) {
                out.push(Addr {
                    text: format!("https://{d}"),
                    url: format!("https://{d}"),
                    up: words::awake_now(i),
                    unready: false,
                    host: String::new(),
                });
            }
        }
    }
    let mut named: Vec<_> = pairs
        .iter()
        .filter(|(v, _)| v.kind != "task" && !v.domain.is_empty())
        .collect();
    named.sort_by_key(|(v, _)| v.kind != "app");
    for (v, i) in named {
        let reach = words::reach(i);
        let addr = reach
            .as_ref()
            .map(|r| format!("{}{}", r.addr, r.port.unwrap_or_default()));
        let web = addr.as_deref().is_some_and(|a| a.starts_with("http"))
            && (v.kind != "module" || !v.url.is_empty() || !v.resource.is_empty());
        out.push(match addr {
            Some(a) if web => Addr {
                text: a.clone(),
                url: a,
                up: words::awake_now(i),
                unready: false,
                // Go: an app's or a service's web name is fronted; a module's is not.
                host: if v.kind == "app" || v.kind == "service" {
                    v.domain.clone()
                } else {
                    String::new()
                },
            },
            _ => Addr {
                text: v.domain.clone(),
                url: String::new(),
                up: words::awake_now(i),
                unready: false,
                host: String::new(),
            },
        });
    }
    let more = out.len().saturating_sub(3);
    out.truncate(3);
    (out, more)
}

#[cfg(test)]
mod tests {
    use super::{Addr, fronted};
    use crate::library::{LibStack, Library};

    fn lib(addresses: Vec<Addr>) -> Library {
        Library {
            stacks: vec![LibStack {
                addresses,
                ..LibStack::default()
            }],
            ..Library::default()
        }
    }

    /// Go `serve/library.go`: an app's pill is the registry's URL for its name, port-less behind the front door; a
    /// module's (no host) and an unready one stay as they were.
    #[test]
    fn an_app_pill_goes_through_the_front_door() {
        let app = Addr {
            text: "http://api.shop.doze:8001".into(),
            url: "http://api.shop.doze:8001".into(),
            up: true,
            host: "api.shop.doze".into(),
            ..Addr::default()
        };
        let module = Addr {
            text: "http://db.shop.doze:5432".into(),
            url: "http://db.shop.doze:5432".into(),
            ..Addr::default()
        };
        let mut l = lib(vec![app, module.clone()]);
        fronted(&mut l, |h| {
            (h == "api.shop.doze").then(|| "http://api.shop.doze".to_owned())
        });
        let a = &l.stacks[0].addresses;
        assert_eq!(
            (a[0].text.as_str(), a[0].url.as_str()),
            ("http://api.shop.doze", "http://api.shop.doze")
        );
        assert_eq!(a[1], module);
    }

    /// Go `AddrPill`'s title.
    #[test]
    fn a_pill_s_title() {
        let unready = Addr {
            text: "https://dev.shop.example".into(),
            unready: true,
            ..Addr::default()
        };
        assert_eq!(
            unready.title(),
            "https://dev.shop.example needs a one-time setup on this machine; the stack's page has it"
        );
        let link = Addr {
            text: "http://api.shop.doze".into(),
            url: "http://api.shop.doze".into(),
            ..Addr::default()
        };
        assert_eq!(link.title(), "Open http://api.shop.doze");
        let name = Addr {
            text: "db.shop.doze".into(),
            ..Addr::default()
        };
        assert_eq!(name.title(), "db.shop.doze");
    }
}
