//! The page's verbs, as the Go page chose them by state: `r` tries a crashing service again, runs a task, restarts an
//! awake one; `s` stops trying, or sleeps; `w` wakes (turning an off service back on first); `p` pins. Restart and
//! sleep of a service in use ask first, and waking looks at its port first: both through `crate::act`, which runs each
//! verb on the hub's short-lived connection, off the UI thread.

use doze_control::Verb;
use doze_display::Look;
use gpui::{Context, Window};

use super::InstanceView;
use crate::theme::tokens;

impl InstanceView {
    /// Runs `verb` at once: what needs no asking (try now, stop trying, pin, run).
    fn run_verbs(&mut self, verbs: Vec<Verb>, cx: &mut Context<Self>) {
        let (key, name) = (self.key.clone(), self.name.clone());
        if let (Some(a), Some(v)) = (crate::act::acts(cx), verbs.first().copied()) {
            a.update(cx, |a, cx| a.run_verbs(&key, v, vec![name], cx));
        }
    }

    /// Asks for `verb` the way the page asks: a question first when one is due, a look before it wakes.
    fn ask(&mut self, verb: Verb, cx: &mut Context<Self>) {
        let (key, name) = (self.key.clone(), self.name.clone());
        if let Some(a) = crate::act::acts(cx) {
            a.update(cx, |a, cx| a.verb(&key, &name, verb, cx));
        }
    }

    pub(crate) fn restart(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if p.is_task() {
            self.run_verbs(vec![Verb::Task], cx);
        } else if p.crash.is_some() {
            self.run_verbs(vec![Verb::Restart], cx);
        } else if p.serving() {
            self.ask(Verb::Restart, cx);
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
            self.ask(Verb::Sleep, cx);
        }
    }

    /// The switch: off brings it back and wakes it; on asks to sleep; else wakes.
    pub(crate) fn flip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = &self.page;
        if p.off {
            self.ask(Verb::On, cx);
        } else if p.on {
            self.sleep(window, cx);
        } else {
            self.ask(Verb::Wake, cx);
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
