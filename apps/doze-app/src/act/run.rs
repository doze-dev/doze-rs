//! Running what was asked: a service's verb on the daemon (Go `Do`, `DoAll`), a stack's action through the CLI (Go
//! `Stack`), a module's action (Go `Admin`), a folder set up (Go `Setup`), and a dialog's button.

use std::path::{Path, PathBuf};

use doze_control::Verb;
use doze_desk::StackKey;
use doze_desk::act::{Button, StackAction, dialog_of};
use doze_display as words;
use gpui::Context;

use super::{Acts, ActsEvent, Ask};

/// What the app says when it has no doze to run.
const NO_DOZE: &str = "doze is not on this Mac's PATH, so the app cannot start or stop stacks. Install doze, then try again; from a terminal: doze up";

impl Acts {
    /// A verb on the service `name`, as its switch, its button or its key asks: the question first when doze-desk says
    /// one is due, the look before it wakes, then the daemon.
    pub(crate) fn verb(&mut self, key: &StackKey, name: &str, verb: Verb, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        if !st.link.is_live() {
            // Go's refusal: dialling would hang and then say something worse.
            self.say(
                format!(
                    "{} {name}: the stack's daemon is not answering, so nothing was sent",
                    verb.as_str()
                ),
                true,
                cx,
            );
            return;
        }
        if name.is_empty() {
            // The whole stack ("Sleep everything in shop"): the daemon takes no target as every one, as Go's `Do`.
            self.run_verbs(key, verb, vec![String::new()], cx);
            return;
        }
        match verb {
            Verb::Sleep | Verb::Restart | Verb::Reset | Verb::Off => {
                let d = dialog_of(&st, name, verb.as_str(), words::home().as_deref());
                if d.needed {
                    self.ask(Some(key), d, cx);
                } else {
                    self.run_verbs(key, verb, vec![name.to_owned()], cx);
                }
            }
            Verb::Wake | Verb::On => self.check_wake(key, name, verb, cx),
            _ => self.run_verbs(key, verb, vec![name.to_owned()], cx),
        }
    }

    /// Runs `verb` on each of `targets` in order, stopping at the first failure ("Sleep db and api" sleeps api, then
    /// db). `on` is followed by `wake`, as the switch does.
    pub(crate) fn run_verbs(
        &mut self,
        key: &StackKey,
        verb: Verb,
        targets: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let busy: Vec<(StackKey, String)> =
            targets.iter().map(|t| (key.clone(), t.clone())).collect();
        self.doing.extend(busy.iter().map(|b| (b.clone(), verb)));
        if verb == Verb::Task
            && let Some(t) = targets.first()
        {
            self.say(words::task_started(t), false, cx);
        }
        cx.notify();
        let (hub, k) = (self.hub.clone(), key.clone());
        cx.spawn(async move |this, cx| {
            let failed = cx
                .background_executor()
                .spawn(async move {
                    for t in &targets {
                        let mut verbs = vec![verb];
                        if verb == Verb::On {
                            verbs.push(Verb::Wake);
                        }
                        for v in verbs {
                            if let Err(e) = hub.verb(&k, v, t) {
                                let what = format!("{} {t}", v.as_str());
                                return Some(format!(
                                    "{} failed: {}",
                                    what.trim_end(),
                                    said(&e, v)
                                ));
                            }
                        }
                    }
                    None
                })
                .await;
            let _ = this.update(cx, |a, cx| {
                for b in &busy {
                    a.doing.remove(b);
                }
                a.hub.refresh();
                match failed {
                    Some(f) => a.say(f, true, cx),
                    None => cx.notify(),
                }
            });
        })
        .detach();
    }

    /// Runs a stack's action now: busy while it runs, its failure kept on the stack's page.
    pub(crate) fn run_stack(
        &mut self,
        key: &StackKey,
        action: StackAction,
        cx: &mut Context<Self>,
    ) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        if self.busy.contains_key(key) {
            return;
        }
        let cli = self.cli.clone();
        if cli.is_none() && action != StackAction::Remove {
            self.say(NO_DOZE, true, cx);
            return;
        }
        self.busy.insert(key.clone(), action.busy());
        self.failed.remove(key);
        // "bringing acme up…" says it is under way, and stays its usual time whenever the action ends (Go: the
        // service's say, a toast like any other).
        self.say(action.said(&st.name), false, cx);
        let (k, name, dir) = (key.clone(), st.name.clone(), PathBuf::from(&st.dir));
        cx.spawn(async move |this, cx| {
            let job = cx.background_executor().spawn({
                let k = k.clone();
                async move {
                    match (&action, cli) {
                        (StackAction::Remove, _) => doze_desk::act::remove(&k, &name),
                        (_, Some(cli)) => cli.stack(&action, &name, &dir).map(|()| String::new()),
                        (_, None) => Ok(String::new()),
                    }
                }
            });
            let done = job.await;
            let _ = this.update(cx, |a, cx| {
                a.busy.remove(&k);
                a.hub.refresh();
                match done {
                    Ok(said) if !said.is_empty() => a.say(said, false, cx),
                    Ok(_) => cx.notify(),
                    Err(f) => {
                        // The page's banner keeps it and Needs you alerts it, from anywhere (as Go); the line is kept
                        // for Recent, with no third toast.
                        a.notices.remember(f.said(), std::time::SystemTime::now());
                        a.failed.insert(k, f);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// A module's action on `inst`: input or a question first when it asks for one, then the daemon.
    pub(crate) fn action(
        &mut self,
        key: &StackKey,
        inst: &str,
        a: &doze_desk::page::console::Action,
        cx: &mut Context<Self>,
    ) {
        let d = doze_desk::act::action_dialog(inst, a);
        if d.needed {
            self.ask(Some(key), d, cx);
        } else {
            self.admin(
                key,
                inst,
                (&a.id, &a.label),
                (&a.resource, String::new()),
                cx,
            );
        }
    }

    /// Runs a module's action and says what it answered (Go `Admin`).
    pub(crate) fn admin(
        &mut self,
        key: &StackKey,
        inst: &str,
        (id, label): (&str, &str),
        (resource, input): (&str, String),
        cx: &mut Context<Self>,
    ) {
        let (hub, k, inst, id, label, resource) = (
            self.hub.clone(),
            key.clone(),
            inst.to_owned(),
            id.to_owned(),
            label.to_owned(),
            resource.to_owned(),
        );
        cx.spawn(async move |this, cx| {
            let said = cx
                .background_executor()
                .spawn(async move {
                    match hub.admin(&k, &inst, &id, &resource, &input) {
                        Ok(out) if out.is_empty() => (format!("{label} done"), false),
                        Ok(out) => (out, false),
                        Err(e) => (
                            format!("{label}: {}", words::humanise(&e.to_string())),
                            true,
                        ),
                    }
                })
                .await;
            let _ = this.update(cx, |a, cx| a.say(said.0, said.1, cx));
        })
        .detach();
    }

    /// Presses a dialog's button: its verb on its targets, a stack's action, a module's action with what was typed,
    /// or a folder set up with the name typed.
    pub(crate) fn choose(&mut self, ask: &Ask, b: &Button, text: String, cx: &mut Context<Self>) {
        let d = &ask.dialog;
        if !d.setup.is_empty() {
            self.setup(Path::new(&d.setup), &b.verb, text, cx);
            return;
        }
        let Some(key) = &ask.key else { return };
        if d.stack {
            if let Some(a) = StackAction::parse(&b.verb, &[]) {
                self.run_stack(key, a, cx);
            }
        } else if d.action {
            let resource = b.targets.first().cloned().unwrap_or_default();
            self.admin(key, &d.instance, (&b.verb, &d.title), (&resource, text), cx);
        } else if let Some(v) = Verb::parse(&b.verb) {
            self.run_verbs(key, v, b.targets.clone(), cx);
        }
    }

    /// Sets `dir` up the way chosen, then shows its stack.
    pub(crate) fn setup(&mut self, dir: &Path, way: &str, arg: String, cx: &mut Context<Self>) {
        let Some(cli) = self.cli.clone() else {
            self.say(NO_DOZE, true, cx);
            return;
        };
        let (dir, way) = (dir.to_path_buf(), way.to_owned());
        self.writing = Some((dir.clone(), way.clone()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let d = dir.clone();
            let done = cx
                .background_executor()
                .spawn(async move { doze_desk::act::setup(&cli, &d, &way, &arg) })
                .await;
            let _ = this.update(cx, |a, cx| {
                a.writing = None;
                a.hub.refresh();
                match done {
                    Ok(said) => {
                        a.say(said, false, cx);
                        cx.emit(ActsEvent::Open(dir));
                    }
                    Err(e) => a.say(e, true, cx),
                }
            });
        })
        .detach();
    }
}

/// A verb's failure in a sentence: the daemon's refusal as Go words it, an older daemon as Go explains it.
fn said(e: &doze_desk::Error, v: Verb) -> String {
    let text = e.to_string();
    if text.contains("unknown op") {
        return format!(
            "the stack's daemon is from an older doze and does not know {}; bring the stack down and up to use it",
            v.as_str()
        );
    }
    words::humanise(&text).to_owned()
}
