//! Doing what ⌘K's row says (Go store's `pick` and `appCommand`): go to a page, open a URL or a folder, a terminal, an
//! app command; a verb or a stack action goes to acting ([`super::wire`]), as the pages' own buttons do.

use doze_desk::StackKey;
use doze_desk::palette::{DoKind, Doing};
use gpui::{Context, Window};

use super::{Root, Screen};
use crate::actions::{OpenBell, ShowHelp, StackVerb, Verb};
use crate::theme::{Choice, Density, Mode, Palette as Colors};

impl Root {
    /// Does what a row of ⌘K (or a fix of Needs you) says.
    pub(crate) fn run_doing(&mut self, d: &Doing, window: &mut Window, cx: &mut Context<Self>) {
        match d.kind {
            DoKind::Page => self.open_page(d, window, cx),
            DoKind::Copy => {
                crate::outward::copy(cx, &d.text);
                self.say(format!("Copied {}", d.text), cx);
            }
            DoKind::Open => match d.text.strip_prefix("file://") {
                Some(path) => crate::outward::open_path(cx, std::path::Path::new(path)),
                None => crate::outward::open_url(cx, &d.text),
            },
            DoKind::Terminal => self.terminal(&d.text, cx),
            DoKind::Verb => {
                let v = Verb {
                    stack: d.stack_key.clone(),
                    verb: d.verb.clone(),
                    target: d.target.clone(),
                    ask: d.ask,
                };
                self.do_verb(&v, cx);
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
                self.do_stack_verb(&v, cx);
            }
            DoKind::Folder => self.pick_folder(false, cx),
            DoKind::App => self.app_command(&d.text, window, cx),
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
            ("activity", _) => {
                self.go(Screen::Activity(key), window, cx);
                if let Some(a) = self.pages.activity.clone() {
                    let only = d.target.clone();
                    a.update(cx, |a, cx| {
                        if a.only != only {
                            a.set_only(&only, cx);
                        }
                    });
                }
                return;
            }
            ("settings", _) => {
                let at = crate::settings::Section::parse(&d.target);
                self.open_settings(at, false, window, cx);
            }
            (other, _) => {
                self.say(format!("doze has no page called “{other}”"), cx);
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

    /// A terminal in a folder: Terminal on macOS, the desktop's own elsewhere (Go `OpenTerminal`).
    fn terminal(&mut self, dir: &str, cx: &mut Context<Self>) {
        if !std::path::Path::new(dir).is_dir() {
            self.say(format!("{dir} is not a folder"), cx);
            return;
        }
        #[cfg(target_os = "macos")]
        let ok = std::process::Command::new("open")
            .args(["-a", "Terminal", dir])
            .spawn()
            .is_ok();
        #[cfg(not(target_os = "macos"))]
        let ok = std::process::Command::new("x-terminal-emulator")
            .current_dir(dir)
            .spawn()
            .is_ok();
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
            "new" => self.pick_folder(true, cx),
            "version" => {
                crate::outward::copy(cx, crate::VERSION);
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
            "settings" => {
                let at = crate::settings::Section::parse(arg);
                self.open_settings(at, false, window, cx);
            }
            // Open at login: on / off, shown where it lives (Settings › General) as it flips.
            "login" => {
                self.open_settings(Some(crate::settings::Section::General), false, window, cx);
                if let Some(s) = self.pages.settings.clone() {
                    let on = arg == "on";
                    s.update(cx, |s, cx| {
                        if s.login != on {
                            s.set_login(cx);
                        }
                    });
                }
            }
            "repair" => window.dispatch_action(Box::new(crate::actions::RepairMachine), cx),
            "updates" => window.dispatch_action(Box::new(crate::actions::CheckUpdates), cx),
            "cleanup" => {
                self.open_settings(Some(crate::settings::Section::Machine), false, window, cx);
            }
            other => self.say(format!("doze does not know “{other}”"), cx),
        }
    }
}
