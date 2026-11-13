//! The way back (Go's history: 50 places, ⌘[ ⌘]), the tabs by number (⌘1–⌘9, counting the strip's stack tabs), the
//! services opened lately (for ⌘K's Recent), and the window-wide commands of the menu bar that are not a page's.

use gpui::{Context, Window};

use super::{Root, Screen};
use crate::actions::{
    AboutDoze, Back, EditConfig, Forward, FullScreen, GoTab, HideDoze, HideOthers, Minimize,
    OpenDocs, RowLogs, ShowAll, ShowHelp, Zoom,
};

/// How many places back are kept.
const KEPT: usize = 50;

/// Where the docs about the app are.
const DOCS: &str = "https://doze-dev.com/docs/doze/app/library/";

/// The places before and after.
#[derive(Debug, Default)]
pub(crate) struct History {
    back: Vec<Screen>,
    ahead: Vec<Screen>,
    /// A step back or forward is under way: the move it makes is not a new place.
    stepping: bool,
}

impl History {
    /// The window moved from `from` to `to`.
    pub(super) fn went(&mut self, from: &Screen, to: &Screen) {
        if self.stepping || from == to {
            return;
        }
        self.back.push(from.clone());
        if self.back.len() > KEPT {
            self.back.remove(0);
        }
        self.ahead.clear();
    }
}

impl Root {
    /// Notes a move: the history, and a service's page for ⌘K's Recent (Go `SetFocus`).
    pub(super) fn moved(&mut self, to: &Screen) {
        self.hist.went(&self.screen, to);
        if let Screen::Instance(k, name) = to {
            crate::prefs::recall::opened(&k.dir().to_string_lossy(), name);
        }
        // Where the window opens next time (Go `doze.last`).
        let dir = |k: &doze_desk::StackKey| k.dir().to_string_lossy().into_owned();
        match to {
            Screen::Stack(k) => crate::prefs::recall::save_last(Some((&dir(k), "home", ""))),
            Screen::Instance(k, n) => {
                crate::prefs::recall::save_last(Some((&dir(k), "instance", n)))
            }
            Screen::Logs(k) => crate::prefs::recall::save_last(Some((&dir(k), "logs", ""))),
            Screen::Library => crate::prefs::recall::save_last(None),
            Screen::Empty(_) => {}
        }
    }

    /// The page the window last showed, while its stack is still known (Go: reopen where I was); else the Library.
    pub(crate) fn reopened(&self) -> Screen {
        let Some((key, page, instance)) = crate::prefs::recall::last() else {
            return Screen::Library;
        };
        let key = doze_desk::StackKey::new(&key);
        if self.hub.stack(&key).is_none() {
            return Screen::Library;
        }
        match page.as_str() {
            "instance" if !instance.is_empty() => Screen::Instance(key, instance),
            "logs" => Screen::Logs(key),
            _ => Screen::Stack(key),
        }
    }

    fn step(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let next = if back {
            self.hist.back.pop()
        } else {
            self.hist.ahead.pop()
        };
        let Some(to) = next else { return };
        let here = self.screen.clone();
        if back {
            self.hist.ahead.push(here);
        } else {
            self.hist.back.push(here);
        }
        self.hist.stepping = true;
        self.go(to, window, cx);
        self.hist.stepping = false;
    }

    /// The stack tab `n` (1–9), as the strip numbers them.
    fn tab_n(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(t) = n.checked_sub(1).and_then(|i| self.tabs.get(i)) {
            let key = t.key.clone();
            self.go(Screen::Stack(key), window, cx);
        }
    }

    /// `l` on a stack's page: the logs of the row selected (the Grid's row under the keys).
    fn row_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let o = self.overview.read(cx);
        let name = if o.view == crate::overview::View::Grid {
            o.cur
                .and_then(|(r, _)| o.home.grid.rows.get(r))
                .map(|r| r.name.clone())
        } else {
            o.selected.clone()
        };
        if let Some(n) = name {
            self.open_instance(&n, window, cx);
            self.open_tab(Some(doze_display::TAB_LOGS), window, cx);
        }
    }

    /// The stack's doze.hcl, in the editor the system opens it with (D9: editing stays in your editor).
    fn edit_config(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.key().and_then(|k| self.hub.stack(k)) else {
            self.say("Open a stack first: ⌘E opens its doze.hcl.", cx);
            return;
        };
        let file = std::path::Path::new(&st.dir).join("doze.hcl");
        if file.is_file() {
            crate::outward::open_path(cx, &file);
        } else {
            self.say(format!("{} has no doze.hcl.", st.dir), cx);
        }
    }

    /// The window-wide commands, on the root.
    pub(super) fn on_history(&mut self, d: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use gpui::InteractiveElement as _;
        d.on_action(cx.listener(|this, _: &Back, window, cx| this.step(true, window, cx)))
            .on_action(cx.listener(|this, _: &Forward, window, cx| this.step(false, window, cx)))
            .on_action(cx.listener(|this, a: &GoTab, window, cx| this.tab_n(a.n, window, cx)))
            .on_action(cx.listener(|this, _: &EditConfig, _, cx| this.edit_config(cx)))
            .on_action(cx.listener(|this, _: &RowLogs, window, cx| this.row_logs(window, cx)))
            .on_action(
                cx.listener(|this, _: &AboutDoze, window, cx| this.on_help(&ShowHelp, window, cx)),
            )
            .on_action(cx.listener(|_, _: &OpenDocs, _, cx| crate::outward::open_url(cx, DOCS)))
            .on_action(cx.listener(|_, _: &HideDoze, _, cx| cx.hide()))
            .on_action(cx.listener(|_, _: &HideOthers, _, cx| cx.hide_other_apps()))
            .on_action(cx.listener(|_, _: &ShowAll, _, cx| cx.unhide_other_apps()))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &Zoom, window, _| window.zoom_window()))
            .on_action(cx.listener(|_, _: &FullScreen, window, _| window.toggle_fullscreen()))
    }
}

#[cfg(test)]
mod tests {
    use super::{History, KEPT, Screen};

    #[test]
    fn history_keeps_fifty_and_a_new_place_drops_what_was_ahead() {
        let mut h = History::default();
        for _ in 0..KEPT + 5 {
            h.went(&Screen::Library, &Screen::Library);
        }
        assert!(h.back.is_empty(), "staying put is not a place");
        let k = doze_desk::StackKey::new("k");
        for _ in 0..KEPT + 5 {
            h.went(&Screen::Library, &Screen::Stack(k.clone()));
        }
        assert_eq!(h.back.len(), KEPT);
        h.ahead.push(Screen::Library);
        h.went(&Screen::Library, &Screen::Logs(k));
        assert!(h.ahead.is_empty());
    }
}
