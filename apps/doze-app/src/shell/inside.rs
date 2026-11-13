//! The pages inside a stack: an instance's page and All logs ([`Screen::Instance`], [`Screen::Logs`]), each beside the
//! sidebar, in place of the stack's page. The stack page stays one Esc away: Esc first lets go of what is inside the
//! page (the filter box, picked lines), then goes back out.

use std::collections::BTreeSet;

use doze_desk::StackKey;
use doze_desk::page::side::{self, Side};
use gpui::{AnyElement, AppContext as _, Context, Entity, IntoElement, Window, div, prelude::*};

use super::{Root, Screen};
use crate::instance::side::{Mode, Pick, sidebar};
use crate::instance::{InstanceEvent, InstanceView};
use crate::logs::{LogsMode, LogsView};
use crate::theme::Tokens;

/// The view the inside screen shows.
pub(crate) enum Inside {
    Instance(Entity<InstanceView>),
    Logs(Entity<LogsView>),
}

impl Root {
    /// Opens the page of the instance `name` of the stack shown.
    pub(crate) fn open_instance(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.key().cloned() else {
            return;
        };
        self.go(Screen::Instance(key, name.to_owned()), window, cx);
    }

    /// Turns the instance page shown to its tab `tab`, if one is named.
    pub(crate) fn open_tab(
        &mut self,
        tab: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(tab), Some(Inside::Instance(v))) = (tab, &self.inside) {
            v.update(cx, |v, cx| v.set_tab(tab, window, cx));
        }
    }

    /// Opens All logs: every instance of the stack shown, merged.
    pub(crate) fn open_logs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.key().cloned() else {
            return;
        };
        self.go(Screen::Logs(key), window, cx);
    }

    /// Makes the instance page for [`Screen::Instance`]; false when the stack has no such instance.
    pub(super) fn enter_instance(
        &mut self,
        key: &StackKey,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(st) = self.hub.stack(key) else {
            return false;
        };
        if st.instance(name).is_none() {
            return false;
        }
        if let Some(Inside::Instance(v)) = &self.inside
            && v.read(cx).name == name
            && v.read(cx).key == *key
        {
            return true;
        }
        let hub = self.hub.clone();
        let k = key.clone();
        let v = cx.new(|cx| InstanceView::new(hub, k, name, cx));
        cx.subscribe_in(
            &v,
            window,
            |this, _, ev: &InstanceEvent, window, cx| match ev {
                InstanceEvent::Open(n) => this.open_instance(n, window, cx),
                InstanceEvent::Home => {
                    this.leave_inside(window, cx);
                }
                InstanceEvent::Say(s) => this.say(s.clone(), cx),
            },
        )
        .detach();
        v.update(cx, |v, cx| v.focus_tab(window, cx));
        self.inside = Some(Inside::Instance(v));
        true
    }

    /// Makes the All logs page for [`Screen::Logs`].
    pub(super) fn enter_logs(
        &mut self,
        key: &StackKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.inside, Some(Inside::Logs(_)))
            && matches!(&self.screen, Screen::Logs(k) if k == key)
        {
            return;
        }
        let hub = self.hub.clone();
        let k = key.clone();
        let v = cx.new(|cx| LogsView::new(hub, k, "", LogsMode::All, cx));
        let f = v.read(cx).focus.clone();
        window.focus(&f, cx);
        self.inside = Some(Inside::Logs(v));
    }

    /// Back to the stack's page; says whether there was a page to leave.
    pub(crate) fn leave_inside(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = match &self.screen {
            Screen::Instance(k, _) | Screen::Logs(k) => k.clone(),
            _ => return false,
        };
        self.go(Screen::Stack(key), window, cx);
        true
    }

    /// Esc inside: the page lets go of what it holds first, else the page closes.
    pub(crate) fn escape_inside(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let held = match &self.inside {
            None => return false,
            Some(Inside::Logs(l)) => l.update(cx, |l, cx| l.escape(window, cx)),
            Some(Inside::Instance(v)) => v.update(cx, |v, cx| v.escape(window, cx)),
        };
        held || self.leave_inside(window, cx)
    }

    /// Gives the page inside the keys again (after an overlay closed); says whether there is one.
    pub(crate) fn focus_inside(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match &self.inside {
            None => false,
            Some(Inside::Instance(v)) => {
                v.update(cx, |v, cx| v.focus_tab(window, cx));
                true
            }
            Some(Inside::Logs(l)) => {
                let f = l.read(cx).focus.clone();
                window.focus(&f, cx);
                true
            }
        }
    }

    /// A batch from the hub, for the page inside.
    pub(crate) fn apply_inside(&mut self, batch: &doze_desk::Batch, cx: &mut Context<Self>) {
        match &self.inside {
            Some(Inside::Instance(v)) => v.update(cx, |v, cx| v.apply(batch, cx)),
            Some(Inside::Logs(l)) => l.update(cx, |l, cx| l.apply(batch, cx)),
            None => {}
        }
    }

    fn sides(&self) -> Vec<Side> {
        self.key()
            .and_then(|k| self.hub.stack(k))
            .map(|s| side::side(&s.words()))
            .unwrap_or_default()
    }

    /// The page inside, beside the sidebar; `None` when no page inside shows.
    pub(crate) fn inside_el(&mut self, t: &Tokens, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sides = self.sides();
        let foot = super::nav::foot(&self.hub);
        let (side, page): (AnyElement, AnyElement) = match &self.inside {
            None => return None,
            Some(Inside::Instance(v)) => {
                let name = v.read(cx).name.clone();
                (
                    sidebar(t, &sides, Mode::Instance(&name), &foot, cx, Root::side_pick)
                        .into_any_element(),
                    v.clone().into_any_element(),
                )
            }
            Some(Inside::Logs(l)) => {
                let show = l.read(cx).show.clone();
                (
                    sidebar(t, &sides, Mode::Show(&show), &foot, cx, Root::side_pick)
                        .into_any_element(),
                    l.clone().into_any_element(),
                )
            }
        };
        Some(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(side)
                .child(div().flex().flex_col().flex_1().min_w_0().child(page))
                .into_any_element(),
        )
    }

    fn side_pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::Overview => {
                self.leave_inside(window, cx);
            }
            Pick::Instance(n) => self.open_instance(&n, window, cx),
            Pick::Toggle(n) => {
                let Some(Inside::Logs(l)) = &self.inside else {
                    return;
                };
                let mut all: Vec<String> = self.sides().into_iter().map(|s| s.name).collect();
                all.push("doze".into());
                let l = l.clone();
                l.update(cx, |l, cx| {
                    let on = |x: &str| l.show.as_ref().is_none_or(|s| s.contains(x));
                    let next: BTreeSet<String> = all
                        .iter()
                        .filter(|x| if **x == n { !on(x) } else { on(x) })
                        .cloned()
                        .collect();
                    let show = (next.len() != all.len()).then_some(next);
                    l.set_show(show, cx);
                });
            }
        }
    }
}

/// The pages inside, for `shot` and `bench` to set up.
#[cfg(feature = "fixtures")]
impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn inside_instance(&self) -> Option<Entity<InstanceView>> {
        match &self.inside {
            Some(Inside::Instance(v)) => Some(v.clone()),
            _ => None,
        }
    }

    pub(crate) fn inside_logs(&self, cx: &gpui::App) -> Option<Entity<LogsView>> {
        match &self.inside {
            Some(Inside::Logs(l)) => Some(l.clone()),
            Some(Inside::Instance(v)) => v.read(cx).output(),
            None => None,
        }
    }
}
