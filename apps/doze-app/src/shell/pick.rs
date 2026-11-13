//! Doing what ⌘K's row says (Go store's `pick` and `appCommand`): go to a page, open a URL or a folder, a terminal, an
//! app command; a verb or a stack action goes to the verbs (the `Verb` and `StackVerb` intents, whose handlers are the
//! verbs' step), and until they are there the page says so rather than pretending.

use doze_desk::StackKey;
use doze_desk::palette::{DoKind, Doing};
use gpui::{Action, Context, Window};

use super::{Root, Screen};
use crate::actions::{OpenBell, OpenFolder, ShowHelp, StackVerb, Verb};
use crate::theme::{Choice, Density, Mode, Palette as Colors};

impl Root {
    /// Does what a row of ⌘K (or a fix of Needs you) says.
    pub(crate) fn run_doing(&mut self, d: &Doing, window: &mut Window, cx: &mut Context<Self>) {
        match d.kind {
            DoKind::Page => self.open_page(d, window, cx),
            DoKind::Copy => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(d.text.clone()));
                self.say(format!("Copied {}", d.text), cx);
            }
            DoKind::Open => match d.text.strip_prefix("file://") {
                Some(path) => cx.open_with_system(std::path::Path::new(path)),
                None => cx.open_url(&d.text),
            },
            DoKind::Terminal => self.terminal(&d.text, cx),
            DoKind::Verb => {
                let v = Verb {
                    stack: d.stack_key.clone(),
                    verb: d.verb.clone(),
                    target: d.target.clone(),
                    ask: d.ask,
                };
                self.intent(Box::new(v), &format!("{} {}", d.verb, d.target), window, cx);
            }
            DoKind::Stack | DoKind::Choose => {
                let verb = if d.kind == DoKind::Choose {
                    "choose".into()
                } else {
                    d.verb.clone()
                };
                let v = StackVerb {
                    stack: d.stack_key.clone(),
                    verb,
                    target: d.target.clone(),
                };
                self.intent(Box::new(v), &d.verb, window, cx);
            }
            DoKind::Folder => self.intent(Box::new(OpenFolder), "Open folder", window, cx),
            DoKind::App => self.app_command(&d.text, window, cx),
        }
    }

    /// Hands an intent to its handler, or says it is not here yet.
    pub(crate) fn intent(
        &mut self,
        a: Box<dyn Action>,
        what: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.is_action_available(a.as_ref(), cx) {
            window.dispatch_action(a, cx);
        } else {
            self.say(
                format!(
                    "“{}” comes with the verbs step; this build looks, it does not touch.",
                    what.trim()
                ),
                cx,
            );
        }
    }

    /// A page: a stack's overview, a service's page, logs (filtered), activity.
    fn open_page(&mut self, d: &Doing, window: &mut Window, cx: &mut Context<Self>) {
        let key = StackKey::new(&d.stack_key);
        match (d.page.as_str(), d.target.is_empty()) {
            ("home", _) => self.go(Screen::Stack(key), window, cx),
            ("instance", false) => self.go(Screen::Instance(key, d.target.clone()), window, cx),
            ("logs", true) => self.go(Screen::Logs(key), window, cx),
            ("logs", false) => {
                self.go(Screen::Instance(key, d.target.clone()), window, cx);
                self.open_tab(Some("logs"), window, cx);
            }
            _ => {
                self.say("Activity comes with a later step (M3).", cx);
                return;
            }
        }
        if !d.text.is_empty() {
            self.search_logs(&d.text, window, cx);
        }
    }

    /// The logs shown filtered to `words`.
    fn search_logs(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        use super::inside::Inside;
        let logs = match &self.inside {
            Some(Inside::Logs(l)) => Some(l.clone()),
            Some(Inside::Instance(v)) => v.read(cx).output(),
            None => None,
        };
        if let Some(l) = logs {
            l.update(cx, |l, cx| l.search(words, window, cx));
        }
    }

    /// A terminal in a folder.
    fn terminal(&mut self, dir: &str, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        let ok = std::process::Command::new("open")
            .args(["-a", "Terminal", dir])
            .spawn()
            .is_ok();
        #[cfg(not(target_os = "macos"))]
        let ok = false;
        if !ok {
            self.say(format!("No terminal could be opened in {dir}."), cx);
        }
    }

    /// One of the app's own commands, by Go's id (`home`, `theme:teal`, …).
    fn app_command(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (what, arg) = id.split_once(':').unwrap_or((id, ""));
        let c = *cx.global::<Choice>();
        match what {
            "home" => self.go(Screen::Library, window, cx),
            "do" => {
                if let Some(s) = crate::actions::all().find(|s| s.label == arg) {
                    window.dispatch_action((s.make)(), cx);
                }
            }
            "bell" => window.dispatch_action(Box::new(OpenBell), cx),
            "help" => window.dispatch_action(Box::new(ShowHelp), cx),
            "new" => self.intent(Box::new(OpenFolder), "New stack", window, cx),
            "version" => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(crate::VERSION.into()));
                self.say(format!("Copied doze's version, {}", crate::VERSION), cx);
            }
            "mode" => {
                if let Some(mode) = Mode::parse(arg) {
                    self.set_theme(Choice { mode, ..c }, window, cx);
                }
            }
            "theme" => {
                if let Some(palette) = Colors::parse(arg) {
                    self.set_theme(Choice { palette, ..c }, window, cx);
                }
            }
            "density" => {
                if let Some(d) = Density::parse(arg) {
                    self.set_density(d, cx);
                }
            }
            _ => self.say(
                "Settings, the machine's setup and updates come with a later step (M3).",
                cx,
            ),
        }
    }
}
