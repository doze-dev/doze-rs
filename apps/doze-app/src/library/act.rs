//! The Library's verbs (Go `library.tsx`): a row's switch is Up as last time or Down (which asks when something is in
//! use); ⌫ removes the stack from doze (always asked); the ⋯ menu restarts, starts everything, opens the start sheet.
//! Each goes through `crate::act`, which marks the stack busy while it runs; the row says so ("starting…").

use doze_desk::StackKey;
use doze_desk::act::{Busy, StackAction};
use gpui::{App, Context};

use super::{LibraryEvent, LibraryView};

/// What the stack keyed `key` is busy doing.
pub(super) fn busy(key: &str, cx: &App) -> Option<Busy> {
    let a = crate::act::acts(cx)?;
    a.read(cx).busy_of(&StackKey::new(key))
}

impl LibraryView {
    /// The stack keyed `key`, as the hub keys it.
    fn stack_key(&self, key: &str) -> Option<StackKey> {
        let i = self.lib.stacks.iter().position(|s| s.key == key)?;
        self.keys.get(i).cloned()
    }

    /// Acts on the stack keyed `key`.
    pub(crate) fn act(&mut self, key: &str, action: StackAction, cx: &mut Context<Self>) {
        // The row acted on is the one selected, so the dialog and the selection agree.
        self.selected = Some(key.to_owned());
        self.menu = None;
        cx.notify();
        let (Some(k), Some(acts)) = (self.stack_key(key), crate::act::acts(cx)) else {
            return;
        };
        acts.update(cx, |a, cx| a.stack(&k, action, cx));
    }

    /// The row's switch: down when up, else up as last time. A switch that cannot act (the folder gone, doze.hcl not
    /// loading, a busy stack) is disabled, as Go's is: its hover title says why.
    pub(crate) fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(st) = self.lib.stacks.iter().find(|s| s.key == key) else {
            return;
        };
        if switch_off(st) || busy(key, cx).is_some() {
            return;
        }
        let action = if st.running {
            StackAction::Down
        } else {
            StackAction::UpLast
        };
        self.act(key, action, cx);
    }

    /// Space: the selected row's switch; a stopped stack whose doze.hcl does not load opens its page instead, and a
    /// stack whose folder is gone does nothing (Go `library.tsx`).
    pub(super) fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        let Some(k) = self.selected.clone() else {
            return;
        };
        let Some(st) = self.lib.stacks.iter().find(|s| s.key == k) else {
            return;
        };
        if !st.missing.is_empty() {
            return;
        }
        if !st.running && !st.broken.is_empty() {
            self.open(&k, cx);
            return;
        }
        self.toggle(&k, cx);
    }

    /// ⌫: removes the selected stack from doze, after asking.
    pub(super) fn remove_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(k) = self.selected.clone() {
            self.act(&k, StackAction::Remove, cx);
        }
    }

    /// The start sheet for the stopped stack keyed `key`, over its page.
    pub(crate) fn choose_start(&mut self, key: &str, cx: &mut Context<Self>) {
        self.menu = None;
        let (Some(k), Some(acts)) = (self.stack_key(key), crate::act::acts(cx)) else {
            return;
        };
        cx.emit(LibraryEvent::Open(k.clone()));
        acts.update(cx, |a, cx| a.choose_start(&k, cx));
    }
}

impl super::menu::MenuHost for LibraryView {
    fn menu_act(&mut self, key: &str, action: StackAction, cx: &mut Context<Self>) {
        self.act(key, action, cx);
    }

    fn menu_choose(&mut self, key: &str, cx: &mut Context<Self>) {
        self.choose_start(key, cx);
    }

    fn menu_reveal(&mut self, dir: &str, file: bool, cx: &mut Context<Self>) {
        self.reveal(dir, file, cx);
    }

    fn menu_close(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }
}

/// The row's switch is disabled: its folder is gone, its doze.hcl does not load while stopped, or no folder is known.
pub(super) fn switch_off(st: &doze_desk::library::LibStack) -> bool {
    (!st.running && (!st.missing.is_empty() || !st.broken.is_empty())) || st.dir.is_empty()
}

/// The switch's hover title, in Go's words (`library.tsx`), when it has one.
pub(super) fn switch_title(
    st: &doze_desk::library::LibStack,
    busy: Option<Busy>,
) -> Option<String> {
    if let Some(b) = busy {
        return Some(format!("{}…", b.word()));
    }
    if !st.running && !st.missing.is_empty() {
        return Some(format!(
            "{}, so it cannot start; remove it, or put the folder back",
            st.missing
        ));
    }
    if st.dir.is_empty() {
        return Some(
            "Run by an older doze that does not say its folder: use doze down in that folder"
                .into(),
        );
    }
    (!st.running && !st.broken.is_empty())
        .then(|| "doze.hcl does not load, so it cannot start; open it to see why".into())
}

#[cfg(test)]
mod tests {
    use super::{switch_off, switch_title};
    use doze_desk::library::LibStack;

    #[test]
    fn a_switch_that_cannot_act_says_why_in_go_words() {
        let mut st = LibStack {
            dir: "~/code/shop".into(),
            ..LibStack::default()
        };
        assert!(!switch_off(&st));
        assert_eq!(switch_title(&st, None), None);
        st.broken = "doze.hcl does not load · line 9".into();
        assert!(switch_off(&st));
        assert_eq!(
            switch_title(&st, None).as_deref(),
            Some("doze.hcl does not load, so it cannot start; open it to see why")
        );
        st.missing = "the folder is gone".into();
        assert_eq!(
            switch_title(&st, None).as_deref(),
            Some("the folder is gone, so it cannot start; remove it, or put the folder back")
        );
        st.running = true;
        assert!(!switch_off(&st), "a running stack can always go down");
    }
}
