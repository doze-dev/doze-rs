//! The window's side of acting (M2): what `Acts` raises opens over the page (a dialog, the checks' sheet, the start
//! sheet), the toasts and Recent float at the foot, a folder with no doze.hcl gets its page, and the window's own
//! commands act: Open folder ⌘O, New stack ⌘N, Recent ⌘⇧M, and the stack shown's restart, stop, remove and edit.

use std::path::{Path, PathBuf};

use doze_desk::StackKey;
use doze_desk::act::{StackAction, no_config};
use gpui::{AppContext as _, Context, Entity, IntoElement, PathPromptOptions, Window};

use super::{Root, Screen};
use crate::act::{Acts, ActsEvent};
use crate::empty::EmptyView;
use crate::overlays::dialog::{DialogEvent, DialogView};
use crate::overlays::preflight::{PreflightEvent, PreflightView};
use crate::overlays::start::{StartEvent, StartView};
use crate::overlays::{Overlay, scrim};
use crate::theme::{Motion, tokens};

impl Root {
    /// Hears what `acts` raises.
    pub(super) fn hear(
        &mut self,
        acts: &Entity<Acts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            acts,
            window,
            |this, _, ev: &ActsEvent, window, cx| match ev {
                ActsEvent::Ask(ask) => {
                    let ask = ask.clone();
                    let v = cx.new(|cx| DialogView::new(ask, window, cx));
                    cx.subscribe_in(&v, window, |this, v, ev: &DialogEvent, window, cx| {
                        let ask = v.read(cx).ask.clone();
                        this.close(window, cx);
                        if let DialogEvent::Chose(b, text) = ev {
                            let (b, text) = (b.clone(), text.clone());
                            this.acts.update(cx, |a, cx| a.choose(&ask, &b, text, cx));
                        }
                    })
                    .detach();
                    this.show_overlay(Overlay::Dialog(v), cx);
                }
                ActsEvent::Check(c) => {
                    let v = cx.new(|cx| PreflightView::new(c.clone(), window, cx));
                    cx.subscribe_in(&v, window, |this, v, ev: &PreflightEvent, window, cx| {
                        let check = v.read(cx).check.clone();
                        if matches!(ev, PreflightEvent::Fix(doze_desk::act::FixKind::Edit(_))) {
                            // Editing the file leaves the sheet up.
                        } else {
                            this.close(window, cx);
                        }
                        this.acts.update(cx, |a, cx| match ev {
                            PreflightEvent::Cancel => {}
                            PreflightEvent::Anyway => a.go_ahead(&check, cx),
                            PreflightEvent::Fix(k) => a.fix(&check, k, cx),
                        });
                    })
                    .detach();
                    this.show_overlay(Overlay::Preflight(v), cx);
                }
                ActsEvent::Start(key, sheet) => {
                    let v = cx.new(|cx| StartView::new(key.clone(), sheet.clone(), window, cx));
                    cx.subscribe_in(&v, window, |this, v, ev: &StartEvent, window, cx| {
                        let key = v.read(cx).key.clone();
                        this.close(window, cx);
                        let action = match ev {
                            StartEvent::Cancel => return,
                            StartEvent::These(names) => StackAction::Up(names.clone()),
                            StartEvent::Everything => StackAction::UpAll,
                        };
                        this.acts.update(cx, |a, cx| a.stack(&key, action, cx));
                    })
                    .detach();
                    this.show_overlay(Overlay::Start(v), cx);
                }
                ActsEvent::Open(dir) => this.open_dir(dir, None, window, cx),
                ActsEvent::Repair => {
                    window.dispatch_action(Box::new(crate::actions::RepairMachine), cx);
                }
            },
        )
        .detach();
    }

    pub(super) fn show_overlay(&mut self, o: Overlay, cx: &mut Context<Self>) {
        self.overlay = Some(o);
        cx.notify();
    }

    /// The overlays of acting, centred over the scrim; a press outside cancels.
    pub(super) fn acting_overlay(
        &self,
        o: &Overlay,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let t = tokens(cx);
        let motion = Motion::of(cx);
        let me = cx.entity().downgrade();
        let cancel: scrim::OnScrim = Box::new(move |window, cx| {
            let _ = me.update(cx, |r, cx| r.close(window, cx));
        });
        let (panel, id) = match o {
            Overlay::Dialog(v) => (v.clone().into_any_element(), "dialog"),
            Overlay::Preflight(v) => (v.clone().into_any_element(), "preflight"),
            Overlay::Start(v) => (v.clone().into_any_element(), "start"),
            Overlay::Admin(v) => (v.clone().into_any_element(), "admin"),
            Overlay::Palette(_) | Overlay::Help | Overlay::Bell(_) => return None,
        };
        Some(scrim::center(&t, panel, motion, id, cancel))
    }

    /// The doze home the stacks are under.
    fn home(&self) -> PathBuf {
        self.hub.home().to_path_buf()
    }

    /// A folder doze has no stack for: its empty page when it has no doze.hcl; recorded, stopped, when it has one
    /// (Go `Open`), so it shows and Up is one press.
    pub(super) fn open_new(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if no_config(dir) {
            let acts = self.acts.clone();
            let v = cx.new(|cx| EmptyView::new(dir.to_path_buf(), acts, cx));
            let f = v.read(cx).focus.clone();
            self.empty = Some(v);
            self.go(Screen::Empty(dir.to_path_buf()), window, cx);
            window.focus(&f, cx);
            return;
        }
        if !dir.join("doze.hcl").is_file() {
            self.say(
                format!("{} is not a folder doze can open", dir.display()),
                cx,
            );
            return;
        }
        match doze_desk::act::record(&self.home(), dir) {
            Ok(key) => {
                self.hub.refresh();
                self.go(Screen::Stack(key), window, cx);
            }
            Err(e) => self.say(format!("could not add {}: {e}", dir.display()), cx),
        }
    }

    /// Open folder ⌘O (and New stack ⌘N): the system's folder picker, then that folder's stack or its empty page.
    pub(super) fn pick_folder(&mut self, new: bool, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(if new {
                "Make a stack here".into()
            } else {
                "Open".into()
            }),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |_, cx| cx.emit_open(dir));
        })
        .detach();
    }

    /// The stack shown, for the window's stack commands.
    fn shown(&mut self, cx: &mut Context<Self>) -> Option<StackKey> {
        let k = self.key().cloned();
        if k.is_none() {
            self.say("Open a stack first: its tab, or a row on Home.", cx);
        }
        k
    }

    pub(super) fn stack_action(&mut self, action: StackAction, cx: &mut Context<Self>) {
        if let Some(k) = self.shown(cx) {
            self.acts.update(cx, |a, cx| a.stack(&k, action, cx));
        }
    }
}

/// A folder chosen in the picker goes to the window as an `Open`, the way a folder set up does.
trait EmitOpen {
    fn emit_open(&mut self, dir: PathBuf);
}

impl EmitOpen for gpui::App {
    fn emit_open(&mut self, dir: PathBuf) {
        if let Some(a) = crate::act::acts(self) {
            a.update(self, |_, cx| cx.emit(ActsEvent::Open(dir)));
        }
    }
}

#[cfg_attr(not(feature = "capture"), allow(dead_code))] // WHY: shots type into the dialog shown
impl Root {
    /// The dialog shown, if one is.
    pub(crate) fn dialog(&self) -> Option<Entity<DialogView>> {
        match &self.overlay {
            Some(Overlay::Dialog(d)) => Some(d.clone()),
            _ => None,
        }
    }
}

impl Root {
    /// The hub the window reads.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))] // WHY: shots find stacks by name
    pub(crate) fn hub(&self) -> &doze_desk::Hub {
        &self.hub
    }
}
