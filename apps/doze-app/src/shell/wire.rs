//! The intents ⌘K and Needs you hand on, done by acting (`crate::act`) the way the pages do them: a verb on a service
//! asks first where Go asks (its dialog), Up looks first (the checks before Up), down and remove ask, and stopping a
//! port's holder looks again that it still holds the port. ⌘K and Needs you call these directly; the actions
//! (`Verb`, `StackVerb`) reach the same place from a key or a menu.

use doze_desk::StackKey;
use doze_desk::act::StackAction;
use gpui::{Context, InteractiveElement as _};

use super::Root;
use crate::actions::{
    NewStack, OpenFolder, RemoveShown, RestartStack, ShowRecent, StackVerb, StopStack, Verb,
};

impl Root {
    /// The window's acting commands: the intents, Open folder ⌘O, New stack ⌘N, Recent ⌘⇧M, and the stack shown's
    /// restart, stop and remove.
    pub(super) fn on_acting(&mut self, d: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        d.on_action(cx.listener(|this, v: &Verb, _, cx| this.do_verb(v, cx)))
            .on_action(cx.listener(|this, v: &StackVerb, _, cx| this.do_stack_verb(v, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.pick_folder(false, cx)))
            .on_action(cx.listener(|this, _: &NewStack, _, cx| this.pick_folder(true, cx)))
            .on_action(cx.listener(|this, _: &ShowRecent, _, cx| {
                this.acts.update(cx, |a, cx| a.toggle_recent(cx));
            }))
            .on_action(cx.listener(|this, _: &RestartStack, _, cx| {
                this.stack_action(StackAction::Restart, cx);
            }))
            .on_action(cx.listener(|this, _: &StopStack, _, cx| {
                this.stack_action(StackAction::Down, cx);
            }))
            .on_action(cx.listener(|this, _: &RemoveShown, _, cx| {
                this.stack_action(StackAction::Remove, cx);
            }))
    }

    /// One column at the foot, as Go's `.toasts`: Needs you's alerts, then the toasts; on Home, above the Machine strip.
    pub(super) fn foot_column(&self, cx: &mut Context<Self>) -> gpui::Div {
        use gpui::{ParentElement as _, Styled as _, div, px};
        let bottom = if matches!(self.screen, super::Screen::Library) {
            96.
        } else {
            46.
        };
        // Recent opens at the top of the column: the alerts step aside while it is open.
        let alerts = if self.acts.read(cx).recent {
            Vec::new()
        } else {
            self.attn_els(cx)
        };
        div()
            .absolute()
            .right(px(20.))
            .bottom(px(bottom))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .children(alerts)
            .child(self.toasts.clone())
    }

    /// A verb on a service, or on the whole stack when `target` is empty (Go store's `ask`, then `run`).
    pub(crate) fn do_verb(&mut self, v: &Verb, cx: &mut Context<Self>) {
        let key = StackKey::new(&v.stack);
        let Some(verb) = doze_control::Verb::parse(&v.verb) else {
            self.say(format!("doze does not know “{}”", v.verb), cx);
            return;
        };
        let target = v.target.clone();
        self.acts
            .update(cx, |a, cx| a.verb(&key, &target, verb, cx));
    }

    /// A stack action (Go store's `stackAction`, `askStack`, `chooseStart`, `dismiss`, `stopHolder`).
    pub(crate) fn do_stack_verb(&mut self, v: &StackVerb, cx: &mut Context<Self>) {
        let key = StackKey::new(&v.stack);
        if let Some(pid) = v.verb.strip_prefix("holder:") {
            let Ok(pid) = pid.parse::<i64>() else {
                self.say(format!("“{pid}” is not a process id"), cx);
                return;
            };
            let name = v.target.clone();
            self.acts
                .update(cx, |a, cx| a.stop_holder_for(&key, &name, pid, cx));
            return;
        }
        match v.verb.as_str() {
            "choose" => self.acts.update(cx, |a, cx| a.choose_start(&key, cx)),
            "dismiss" => self.acts.update(cx, |a, cx| a.dismiss_failure(&key, cx)),
            other => match StackAction::parse(other, &[]) {
                Some(action) => self.acts.update(cx, |a, cx| a.stack(&key, action, cx)),
                None => self.say(format!("doze does not know “{other}”"), cx),
            },
        }
    }
}
