//! The page's verbs, as the Go page chose them by state: `r` tries a crashing service again, runs a task, restarts an
//! awake one; `s` stops trying, or sleeps; `w` wakes (turning an off service back on first); `p` pins. Restart and
//! sleep of a service that is serving ask first, and the confirmation dialog is M2's (step 2.2), so until then they
//! say so rather than act unasked. Each verb runs on the hub's short-lived connection, off the UI thread.

use doze_control::Verb;
use doze_display::Look;
use gpui::{Context, Window};

use super::InstanceView;
use crate::theme::tokens;

impl InstanceView {
    fn run_verbs(&mut self, verbs: Vec<Verb>, cx: &mut Context<Self>) {
        let (hub, key, name) = (self.hub.clone(), self.key.clone(), self.name.clone());
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    for v in verbs {
                        hub.verb(&key, v, &name)?;
                    }
                    Ok::<(), doze_desk::Error>(())
                })
                .await;
            if let Err(e) = res {
                let _ = this.update(cx, |v, cx| v.say(e.to_string(), cx));
            }
        })
        .detach();
    }

    fn asks(&self, what: &str, cx: &mut Context<Self>) {
        self.say(
            format!(
                "{what} {} asks first; the confirmation comes with M2. From a terminal: doze {} {}",
                self.name,
                what.to_lowercase(),
                self.name
            ),
            cx,
        );
    }

    pub(crate) fn restart(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if p.is_task() {
            self.run_verbs(vec![Verb::Task], cx);
        } else if p.crash.is_some() {
            self.run_verbs(vec![Verb::Restart], cx);
        } else if p.serving() {
            self.asks("Restart", cx);
        }
    }

    pub(crate) fn sleep(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if p.is_task() || p.paused {
            return;
        }
        if p.crash.is_some() {
            self.run_verbs(vec![Verb::Off], cx);
        } else if p.serving() {
            self.asks("Sleep", cx);
        }
    }

    /// The switch: off brings it back and wakes it; on asks to sleep; else wakes.
    pub(crate) fn flip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if p.off {
            self.run_verbs(vec![Verb::On, Verb::Wake], cx);
        } else if p.on {
            self.sleep(window, cx);
        } else {
            self.run_verbs(vec![Verb::Wake], cx);
        }
    }

    pub(crate) fn wake(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if !p.is_task() && !p.on && !p.paused {
            self.flip(window, cx);
        }
    }

    pub(crate) fn pin(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if !p.is_task() && !p.paused && !p.off {
            self.run_verbs(vec![Verb::Pin], cx);
        }
    }

    pub(crate) fn run_task(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.page.is_task() {
            self.run_verbs(vec![Verb::Task], cx);
        }
    }

    pub(crate) fn open_address(&mut self, cx: &mut Context<Self>) {
        if let (Some(url), true) = (self.page.url(), self.page.serving()) {
            cx.open_url(url);
        }
    }

    pub(crate) fn copy_address(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.page.copy_line().map(str::to_owned) {
            self.copy("address", &line, cx);
            self.say(format!("Copied {line}"), cx);
        }
    }

    /// Opens doze.hcl in the person's editor.
    pub(crate) fn edit_config(&mut self, cx: &mut Context<Self>) {
        let path = format!("file://{}/doze.hcl", self.dir);
        cx.open_url(&path);
    }

    /// The word beside the switch is drawn in its tone; asleep is plain.
    pub(crate) fn word_color(&self, cx: &Context<Self>) -> gpui::Hsla {
        let t = tokens(cx);
        self.page.look.map_or(t.dim, |l: Look| t.tone(l.tone()))
    }
}
