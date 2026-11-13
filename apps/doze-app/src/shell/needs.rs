//! Needs you's doings on the window: the bell's panel opened and closed, a fix pressed (the same intents the pages'
//! verbs take), Show, Not now, an alert closed, the away strip's buttons; and where the panel, the alerts and the strip
//! are drawn.

use doze_desk::StackKey;
use doze_desk::attention::{Fix, FixKind, Item};
use gpui::{
    AnimationExt as _, AnyElement, AppContext as _, Context, Entity, Focusable as _, IntoElement,
    Window, div, prelude::*, px,
};

use super::{Root, Screen};
use crate::actions::{OpenBell, StackVerb, Verb};
use crate::overlays::Overlay;
use crate::overlays::bell::{Bell, BellEvent};
use crate::theme::{Motion, tokens};

impl Root {
    /// Whether the panel is open.
    pub(crate) fn bell_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::Bell(_)))
    }

    /// Opens the panel (or closes it, as a second ⌘⇧N or a click on the bell does).
    pub(crate) fn on_bell(&mut self, _: &OpenBell, window: &mut Window, cx: &mut Context<Self>) {
        if self.bell_open() {
            self.shut(window, cx);
            return;
        }
        let (list, now) = (self.attn.list.clone(), self.hub.now());
        let b = cx.new(|cx| Bell::new(list, now, cx));
        cx.subscribe_in(&b, window, |this, _, ev: &BellEvent, window, cx| match ev {
            BellEvent::Fix(it, f) => this.fix_item(it, f, window, cx),
            BellEvent::Show(it) => this.show_item(it, window, cx),
            BellEvent::NotNow(it) => this.not_now_item(it, cx),
        })
        .detach();
        window.focus(&b.focus_handle(cx), cx);
        self.overlay = Some(Overlay::Bell(b));
        // The bell open, alerts and the away strip are said there.
        self.attn.alerts.clear();
        self.attn.away = None;
        cx.notify();
    }

    /// Closes the panel if it is open (⌘K opening over it).
    pub(super) fn close_bell(&mut self) {
        if self.bell_open() {
            self.overlay = None;
        }
    }

    /// The panel follows a new list.
    pub(super) fn bell_follow(&mut self, cx: &mut Context<Self>) {
        if let Some(Overlay::Bell(b)) = &self.overlay {
            let (list, now) = (self.attn.list.clone(), self.hub.now());
            b.update(cx, |b, cx| b.set(list, now, cx));
        }
    }

    /// Presses a fix.
    pub(crate) fn fix_item(
        &mut self,
        it: &Item,
        f: &Fix,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_alert(&it.id, cx);
        let stack = it.key.clone();
        let target = it.instance.clone();
        match f.kind {
            FixKind::Do => {
                let ask = self
                    .hub
                    .stack(&StackKey::new(&stack))
                    .and_then(|st| {
                        st.words()
                            .find(&target)
                            .map(|v| doze_desk::palette::asks(v, f.verb))
                    })
                    .unwrap_or(false);
                let v = Verb {
                    stack,
                    verb: f.verb.into(),
                    target,
                    ask,
                };
                self.shut_bell(window, cx);
                self.intent(Box::new(v), &f.label, window, cx);
            }
            FixKind::Stack | FixKind::Holder | FixKind::Dismiss => {
                let verb = match f.kind {
                    FixKind::Holder => format!("holder:{}", f.pid),
                    FixKind::Dismiss => "dismiss".into(),
                    _ => f.verb.into(),
                };
                if matches!(f.verb, "remove") || f.kind == FixKind::Dismiss {
                    self.attn.quiet(&stack);
                }
                if f.kind == FixKind::Stack {
                    self.shut_bell(window, cx);
                }
                self.intent(
                    Box::new(StackVerb {
                        stack,
                        verb,
                        target,
                    }),
                    &f.label,
                    window,
                    cx,
                );
            }
            FixKind::Edit => cx.open_with_system(std::path::Path::new(&f.path)),
            FixKind::Copy => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(f.text.clone()));
                self.say("Copied the command", cx);
            }
            FixKind::Show => self.show_item(it, window, cx),
            FixKind::Repair | FixKind::Clean => self.say(
                "Setting up this Mac and cleaning up come with Settings › Machine (M3).",
                cx,
            ),
        }
    }

    fn shut_bell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bell_open() {
            self.shut(window, cx);
        }
    }

    /// Goes to the page that explains an item, and closes the panel.
    pub(crate) fn show_item(&mut self, it: &Item, window: &mut Window, cx: &mut Context<Self>) {
        self.shut_bell(window, cx);
        self.close_alert(&it.id, cx);
        let Some(p) = &it.show else { return };
        let key = StackKey::new(&p.key);
        match p.page {
            "instance" => self.go(Screen::Instance(key, p.instance.clone()), window, cx),
            "home" => self.go(Screen::Stack(key), window, cx),
            _ => self.say("Settings › Machine comes with a later step (M3).", cx),
        }
    }

    fn not_now_item(&mut self, it: &Item, cx: &mut Context<Self>) {
        self.attn.put_aside(it);
        self.rebuild_attention(cx);
    }

    /// Closes an alert; its item stays in Needs you.
    pub(crate) fn close_alert(&mut self, id: &str, cx: &mut Context<Self>) {
        let before = self.attn.alerts.len();
        self.attn.alerts.retain(|a| a.id != id);
        if self.attn.alerts.len() != before {
            cx.notify();
        }
    }

    pub(crate) fn away_to_bell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.attn.away = None;
        if !self.bell_open() {
            self.on_bell(&OpenBell, window, cx);
        }
    }

    pub(crate) fn away_done(&mut self, cx: &mut Context<Self>) {
        self.attn.away = None;
        cx.notify();
    }

    /// An away line: that service's page.
    pub(crate) fn away_open(
        &mut self,
        key: &str,
        inst: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.attn.away = None;
        self.go(
            Screen::Instance(StackKey::new(key), inst.to_owned()),
            window,
            cx,
        );
    }

    /// The panel, under the bell, over the page; a click outside it closes it.
    pub(super) fn bell_el(&self, b: Entity<Bell>, cx: &mut Context<Self>) -> AnyElement {
        let motion = Motion::of(cx);
        let panel = div().absolute().top(px(44.)).right(px(184.)).child(b);
        let panel = if motion.moves() {
            let travel = motion.travel;
            panel
                .with_animation(
                    "bell-in",
                    gpui::Animation::new(motion.normal).with_easing(gpui::ease_out_quint()),
                    move |d, p| d.opacity(p).top(px(44. - travel * (1.0 - p))),
                )
                .into_any_element()
        } else {
            panel.into_any_element()
        };
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .child(
                div()
                    .id("bell-outside")
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .size_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|r, _, window, cx| r.shut_bell(window, cx)),
                    ),
            )
            .child(panel)
            .into_any_element()
    }

    /// The alerts and the away strip, over the page.
    pub(super) fn attn_els(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = tokens(cx);
        let mut out = Vec::new();
        if let Some(a) = self.attn.away.clone() {
            out.push(crate::overlays::alerts::away(&t, &a, cx));
        }
        let screen = self.screen.clone();
        let here = move |it: &Item| match (&it.show, &screen) {
            (Some(p), Screen::Instance(k, n)) => {
                p.page == "instance" && *k.dir().to_string_lossy() == *p.key && *n == p.instance
            }
            (Some(p), Screen::Stack(k)) => p.page == "home" && *k.dir().to_string_lossy() == *p.key,
            _ => false,
        };
        let alerts = self.attn.alerts.clone();
        out.extend(crate::overlays::alerts::alerts(&t, &alerts, &here, cx));
        out.into_iter().map(IntoElement::into_any_element).collect()
    }
}

/// What `shot` stages its overlays with.
#[cfg(feature = "capture")]
impl Root {
    pub(crate) fn overlay_now(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub(crate) fn hub_now(&self) -> std::time::SystemTime {
        self.hub.now()
    }

    pub(crate) fn hub_stacks(&self) -> Vec<doze_desk::StackView> {
        self.hub.stacks()
    }
}
