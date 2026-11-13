//! The PortClash board: the port an app wants is held by something else, named, with the ways out, none of them
//! destructive without a click that says exactly what it does. Stopping the holder sends it a polite TERM, after
//! looking again that it still holds the port, then restarts the app (Go `StopHolder`).

use doze_control::Verb;
use doze_desk::ports;
use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px};

use super::ui::{button, card, heading, step_num, text};
use super::{InstanceEvent, InstanceView};
use crate::theme::{Tokens, ty};

/// The card, once the holder is known.
pub(crate) fn clash(
    t: &Tokens,
    v: &InstanceView,
    cx: &mut Context<InstanceView>,
) -> Option<AnyElement> {
    let h = v.held.as_ref()?;
    let name = &v.name;
    let who = format!(
        "{}, pid {}",
        if h.command.is_empty() {
            "a process"
        } else {
            &h.command
        },
        h.pid
    );
    let first = if h.doze {
        let said = if h.stack.is_empty() {
            "It is another doze program".to_owned()
        } else {
            format!("It is {} in {}", h.service, h.stack)
        };
        option(
            t,
            1,
            &said,
            "Two stacks want the same port. Bring that stack down from Home, or give one of them another port.",
            button(t, "clash-home", "Home", "⌘0", false)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(InstanceEvent::Home)))
                .into_any_element(),
        )
    } else {
        let pid = h.pid;
        let cmd = if h.command.is_empty() {
            "it".to_owned()
        } else {
            h.command.clone()
        };
        option(
            t,
            1,
            &format!("Stop pid {pid} and start {name}"),
            "doze sends it a polite TERM. It is not one of doze's; doze looks again that it still holds the port first.",
            button(t, "clash-stop", &format!("Stop {cmd}"), "", false)
                .on_click(cx.listener(move |v, _, _, cx| v.stop_holder(pid, cx)))
                .into_any_element(),
        )
    };
    let second = option(
        t,
        2,
        "Use another port",
        &format!(
            "Change port = {} in doze.hcl; {name}'s name keeps working on whatever you pick.",
            h.port
        ),
        button(t, "clash-edit", "Edit doze.hcl", "", false)
            .on_click(cx.listener(|v, _, _, cx| v.edit_config(cx)))
            .into_any_element(),
    );
    Some(
        card(t)
            .child(heading(t, &format!("Port {} is taken", h.port), Some(&format!("· {name} cannot listen while {who} has it"))))
            .child(div().flex().flex_col().gap(px(8.)).child(first).child(second))
            .child(div().text_size(px(ty::SM)).text_color(t.dim).child(
                "doze never stops a process it did not start without asking, which is why this is a question and not a thing that happened.",
            ))
            .into_any_element(),
    )
}

/// `.opt`: a numbered way out, what it does, and its button.
fn option(t: &Tokens, n: usize, title: &str, sub: &str, act: AnyElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(8.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .child(step_num(t, n))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_color(t.bright)
                        .font_weight(ty::SEMIBOLD)
                        .child(text(title)),
                )
                .child(
                    div()
                        .text_size(px(ty::SM))
                        .line_height(px(ty::LH_17))
                        .text_color(t.dim)
                        .child(text(sub)),
                ),
        )
        .child(act)
        .into_any_element()
}

impl InstanceView {
    /// Names what holds the port a crash says was taken, off the UI thread (lsof is a fork).
    pub(super) fn look_up_holder(&mut self, port: Option<u16>, cx: &mut Context<Self>) {
        if port == self.held_port {
            return;
        }
        self.held_port = port;
        self.held = None;
        let Some(port) = port else { return };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let held = cx
                .background_executor()
                .spawn(async move {
                    let mut h = ports::holder(port)?;
                    let stacks = hub.stacks();
                    let pids = stacks.iter().flat_map(|s| {
                        s.instances
                            .iter()
                            .map(move |v| (s.name.as_str(), v.name.as_str(), v.pid))
                    });
                    ports::owned_by(&mut h, pids);
                    Some(h)
                })
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.held_port == Some(port) {
                    v.held = held;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    /// Stops the holder `pid` (after looking again) and restarts this app once it has gone.
    fn stop_holder(&mut self, pid: i64, cx: &mut Context<Self>) {
        let Some(port) = self.held.as_ref().map(|h| h.port) else {
            return;
        };
        let (hub, key, name) = (self.hub.clone(), self.key.clone(), self.name.clone());
        cx.spawn(async move |this, cx| {
            let said = cx
                .background_executor()
                .spawn(async move {
                    ports::forget(port);
                    match ports::holder(port) {
                        Some(h) if h.pid == pid && !h.doze => {}
                        Some(h) if h.doze => {
                            return "that is doze's own; bring its stack down from Home instead"
                                .to_owned();
                        }
                        _ => {
                            return "that process no longer holds the port; nothing was stopped"
                                .to_owned();
                        }
                    }
                    let p = u32::try_from(pid).unwrap_or(0);
                    if let Err(e) = doze_os::process::terminate(p) {
                        return format!("could not stop pid {pid}: {e}");
                    }
                    for _ in 0..20 {
                        if !doze_os::process::alive(p) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    ports::forget(port);
                    match hub.verb(&key, Verb::Restart, &name) {
                        Ok(()) => format!("sent TERM to pid {pid}; starting {name}"),
                        Err(e) => format!("stopped pid {pid}; {name} did not restart: {e}"),
                    }
                })
                .await;
            let _ = this.update(cx, |v, cx| v.say(said, cx));
        })
        .detach();
    }
}
