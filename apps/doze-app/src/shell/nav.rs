//! Where the window is: the Library, or a stack's page; and the title bar's tabs (every running stack, and the
//! stopped ones opened this session, as the Go app kept them).

use doze_desk::StackKey;
use doze_display as words;
use gpui::{Context, Focusable, Window};

use super::Root;
use crate::overview::View;

/// The page shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Screen {
    /// The first tab: every stack.
    Library,
    /// A stack's page.
    Stack(StackKey),
    /// An instance's page, beside the stack's sidebar.
    Instance(StackKey, String),
    /// All logs of a stack, merged, beside its sidebar.
    Logs(StackKey),
    /// A folder with no doze.hcl yet: the three ways to start.
    Empty(std::path::PathBuf),
    /// What a stack did over an hour, a day, a week.
    Activity(StackKey),
    /// Settings: General, Machine, About.
    Settings,
}

impl Screen {
    /// The stack the screen is in, if any.
    pub(crate) fn key(&self) -> Option<&StackKey> {
        match self {
            Screen::Library | Screen::Empty(_) | Screen::Settings => None,
            Screen::Stack(k) | Screen::Instance(k, _) | Screen::Logs(k) | Screen::Activity(k) => {
                Some(k)
            }
        }
    }
}

/// A stack's tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tab {
    pub(crate) key: StackKey,
    pub(crate) name: String,
    /// The folder its doze.hcl is in: the tab's hover title.
    pub(crate) dir: String,
    pub(crate) running: bool,
    /// How many of its instances are in trouble, tasks aside (Go: the summary's `troubled`): the tab's red dot and badge.
    pub(crate) troubled: usize,
    /// Its daemon answers (Go: `live`): a running stack out of touch has a yellow dot.
    pub(crate) live: bool,
}

/// The window's title: where the page is (Go app.tsx `titled`), which the Window menu lists and a screen reader
/// reads on every move. `stack` is the shown stack's name.
pub(crate) fn title(screen: &Screen, stack: &str) -> String {
    match screen {
        Screen::Library => "Home · doze".into(),
        Screen::Settings => "Settings · doze".into(),
        Screen::Instance(_, name) => format!("{name} · {stack} · doze"),
        Screen::Logs(_) => format!("All logs · {stack} · doze"),
        Screen::Activity(_) => format!("Activity · {stack} · doze"),
        Screen::Empty(_) => "A folder · doze".into(),
        Screen::Stack(_) if !stack.is_empty() => format!("{stack} · doze"),
        Screen::Stack(_) => "doze".into(),
    }
}

/// The sidebar's foot, as Go's: "doze v0.1.2 · 2 stacks".
pub(super) fn foot(hub: &doze_desk::Hub) -> String {
    let stacks = hub.stacks().len();
    format!(
        "doze {} · {stacks} {}",
        crate::VERSION,
        if stacks == 1 { "stack" } else { "stacks" }
    )
}

impl Root {
    /// Shows `screen`: its page reads the hub, the other stops watching, the keys go to it.
    pub(crate) fn go(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(k) = screen.key() {
            self.reopened_tab(&k.clone());
        }
        match &screen {
            Screen::Instance(k, name) => {
                if !self.enter_instance(k, name, window, cx) {
                    return;
                }
                self.overview
                    .update(cx, |o, cx| o.show(Some(k.clone()), cx));
                self.overview.update(cx, |o, _| o.hide());
            }
            Screen::Logs(k) => {
                self.overview
                    .update(cx, |o, cx| o.show(Some(k.clone()), cx));
                self.overview.update(cx, |o, _| o.hide());
                self.enter_logs(k, window, cx);
            }
            Screen::Stack(k) => {
                self.inside = None;
                let key = k.clone();
                self.overview.update(cx, |o, cx| {
                    o.show(Some(key), cx);
                    o.refresh(cx);
                });
                let o = self.overview.read(cx);
                let f = if o.view == View::Map {
                    o.map.read(cx).focus.clone()
                } else {
                    o.focus.clone()
                };
                window.focus(&f, cx);
            }
            Screen::Empty(_) => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                if let Some(e) = &self.empty {
                    let f = e.read(cx).focus.clone();
                    window.focus(&f, cx);
                }
            }
            Screen::Library => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                self.library.update(cx, |l, cx| l.refresh(cx));
                let f = self.library.focus_handle(cx);
                window.focus(&f, cx);
            }
            Screen::Activity(k) => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                self.enter_activity(k, window, cx);
            }
            Screen::Settings => {
                self.inside = None;
                self.overview.update(cx, |o, _| o.hide());
                self.enter_settings(window, cx);
            }
        }
        self.moved(&screen);
        self.retitle(&screen, window);
        self.screen = screen;
        self.refresh_tabs(cx);
        self.wind(cx);
        cx.notify();
    }

    /// Takes a stack's tab off the strip; the stack keeps running, and its tab stays put away (remembered across
    /// launches) until it is opened again. Closing the tab on screen goes to the tab before it, or after it, or Home
    /// (Go `closeTab`).
    pub(crate) fn close_tab(
        &mut self,
        key: &StackKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = neighbour(&self.tabs, key);
        self.opened.retain(|k| k != key);
        self.hidden.retain(|k| k != key);
        self.hidden.push(key.clone());
        self.keep_closed();
        if self.key() == Some(key) {
            match next {
                Some(k) => self.go(Screen::Stack(k), window, cx),
                None => self.go(Screen::Library, window, cx),
            }
        } else {
            self.refresh_tabs(cx);
        }
    }

    /// A stack's page opens: its tab is in the strip again.
    fn reopened_tab(&mut self, k: &StackKey) {
        if !self.opened.contains(k) {
            self.opened.push(k.clone());
        }
        if self.hidden.contains(k) {
            self.hidden.retain(|h| h != k);
            self.keep_closed();
        }
    }

    /// The window's title for `screen`.
    fn retitle(&self, screen: &Screen, window: &mut Window) {
        let stack = screen
            .key()
            .and_then(|k| self.hub.stack(k))
            .map(|s| s.name)
            .unwrap_or_default();
        window.set_window_title(&title(screen, &stack));
    }

    /// The closed tabs the prefs remember (`closedTabs`).
    pub(super) fn closed_tabs() -> Vec<StackKey> {
        crate::prefs::page::closed_tabs()
            .iter()
            .map(StackKey::new)
            .collect()
    }

    /// Writes the closed tabs to the prefs (`closedTabs`).
    fn keep_closed(&self) {
        let keys: Vec<String> = self
            .hidden
            .iter()
            .map(|k| k.dir().to_string_lossy().into_owned())
            .collect();
        crate::prefs::page::save_closed_tabs(&keys);
    }

    /// The tabs, from the hub; redraws only when they changed.
    pub(crate) fn refresh_tabs(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<Tab> = self
            .hub
            .stacks()
            .into_iter()
            .filter(|s| {
                (s.running && !self.hidden.contains(&s.key)) || self.opened.contains(&s.key)
            })
            .map(|s| {
                let snap = s.words();
                Tab {
                    troubled: snap
                        .instances
                        .iter()
                        .filter(|v| v.kind != doze_display::Kind::Task && words::troubled(v))
                        .count(),
                    key: s.key,
                    name: s.name,
                    dir: s.dir,
                    running: s.running,
                    live: s.link.conn == doze_desk::Conn::Live,
                }
            })
            .collect();
        if tabs != self.tabs {
            self.tabs = tabs;
            cx.notify();
        }
    }
}

/// The tab to show when the tab `key` closes: the one before it, else the one after it (Go `closeTab`).
pub(crate) fn neighbour(tabs: &[Tab], key: &StackKey) -> Option<StackKey> {
    let i = tabs.iter().position(|t| &t.key == key)?;
    i.checked_sub(1)
        .and_then(|j| tabs.get(j))
        .or_else(|| tabs.get(i + 1))
        .map(|t| t.key.clone())
}

#[cfg(test)]
mod tests {
    use super::{Screen, Tab, neighbour, title};
    use doze_desk::StackKey;

    #[test]
    fn the_title_says_where_the_page_is_in_go_words() {
        let k = StackKey::new("/code/shop");
        assert_eq!(title(&Screen::Library, ""), "Home · doze");
        assert_eq!(title(&Screen::Settings, ""), "Settings · doze");
        assert_eq!(title(&Screen::Stack(k.clone()), "shop"), "shop · doze");
        assert_eq!(title(&Screen::Stack(k.clone()), ""), "doze");
        assert_eq!(
            title(&Screen::Instance(k.clone(), "api".into()), "shop"),
            "api · shop · doze"
        );
        assert_eq!(
            title(&Screen::Logs(k.clone()), "shop"),
            "All logs · shop · doze"
        );
        assert_eq!(
            title(&Screen::Activity(k), "shop"),
            "Activity · shop · doze"
        );
        assert_eq!(
            title(&Screen::Empty("/code/new".into()), ""),
            "A folder · doze"
        );
    }

    #[test]
    fn closing_a_tab_goes_to_the_one_before_then_after() {
        let tab = |k: &str| Tab {
            key: StackKey::new(k),
            name: k.into(),
            dir: k.into(),
            running: true,
            troubled: 0,
            live: true,
        };
        let tabs = vec![tab("a"), tab("b"), tab("c")];
        assert_eq!(
            neighbour(&tabs, &StackKey::new("b")),
            Some(StackKey::new("a"))
        );
        assert_eq!(
            neighbour(&tabs, &StackKey::new("a")),
            Some(StackKey::new("b"))
        );
        assert_eq!(neighbour(&tabs[..1], &StackKey::new("a")), None);
        assert_eq!(neighbour(&tabs, &StackKey::new("z")), None);
    }
}
