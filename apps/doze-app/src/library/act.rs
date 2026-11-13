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
        self.menu = None;
        cx.notify();
        let (Some(k), Some(acts)) = (self.stack_key(key), crate::act::acts(cx)) else {
            return;
        };
        acts.update(cx, |a, cx| a.stack(&k, action, cx));
    }

    /// The row's switch: down when up, else up as last time.
    pub(crate) fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(st) = self.lib.stacks.iter().find(|s| s.key == key) else {
            return;
        };
        if !st.missing.is_empty() {
            let words = format!(
                "{}: {}, so it cannot start; remove it, or put the folder back",
                st.name, st.missing
            );
            self.say(&words, cx);
            return;
        }
        if !st.running && !st.broken.is_empty() {
            let words = format!(
                "{}: doze.hcl does not load, so it cannot start; open it to see why",
                st.name
            );
            self.say(&words, cx);
            return;
        }
        let action = if st.running {
            StackAction::Down
        } else {
            StackAction::UpLast
        };
        self.act(key, action, cx);
    }

    /// Space: the selected row's switch.
    pub(super) fn toggle_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(k) = self.selected.clone() {
            self.toggle(&k, cx);
        }
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
