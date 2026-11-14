//! The run sheet: a privileged action from the app (the machine's setup, clearing what stacks left behind, doctor),
//! shown before anything runs, then run with what it prints streamed in, and how it ended said in words.
//!
//! First the plan: each step as it will run (`doze dns-setup` in a folder, a script as root, a pid stopped), with the
//! script a doze command would run as root, asked of the CLI itself (`--print`), and which steps ask for the password.
//! Nothing runs until Run is pressed; each step that needs root then raises the system's own prompt, once. After, doctor
//! and the look for leftovers are asked again, and a repair's words wait for doctor's new report.
//!
//! The commands are the command source's (`doze_desk::settings::cli`): today Go's `doze`. A development build takes a
//! stand-in for the system's prompt from `DOZE_APP_ADMIN_RUNNER` (a script that echoes), so the sheet can be driven on
//! this Mac without raising root.

mod view;

use std::path::{Path, PathBuf};
use std::process::Command;

use doze_desk::Hub;
use doze_desk::settings::machine::parts;
use doze_desk::settings::plan::{self, Ended};
use doze_desk::settings::{Cli, Job, Outcome, Planned, Said, SystemHands};
use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle, Window};

use crate::facts::Facts;

/// Where the sheet is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Asking the CLI what it would run.
    Planning,
    /// Shown; nothing has run.
    Ready,
    /// Running step by step.
    Running,
    /// Every step ran; doctor is looking again.
    Checking,
    /// Over: what it came to, and whether that is bad news.
    Done(String, bool),
}

/// What the sheet tells the window.
#[derive(Debug, Clone)]
pub(crate) enum AdminEvent {
    Close,
    /// A job started or ended (Settings shows "Repairing…" meanwhile).
    Busy(bool),
    /// Says how it ended, as a toast too (the sheet may be closed by then).
    Say(String, bool),
}

/// A step and the script it runs as root, when the CLI printed one.
#[derive(Debug, Clone)]
pub(crate) struct Shown {
    pub(crate) step: Planned,
    pub(crate) asks: bool,
    pub(crate) root: Option<String>,
}

/// The run sheet.
pub(crate) struct AdminView {
    pub(crate) job: Job,
    cli: Option<Cli>,
    hub: Hub,
    facts: Entity<Facts>,
    fixture: bool,
    pub(crate) steps: Vec<Shown>,
    /// What the plan named (routes, leftovers): Run acts on these alone.
    seen: plan::Seen,
    pub(crate) phase: Phase,
    pub(crate) lines: Vec<Said>,
    /// A repair's outcome, waiting for doctor's next answer (after `seen`).
    waiting: Option<(Outcome, u64)>,
    pub(crate) scroll: ScrollHandle,
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<AdminEvent> for AdminView {}

impl Focusable for AdminView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The stand-in for the system's prompt, in a development build.
fn admin_runner() -> Option<PathBuf> {
    #[cfg(feature = "fixtures")]
    {
        std::env::var_os("DOZE_APP_ADMIN_RUNNER").map(PathBuf::from)
    }
    #[cfg(not(feature = "fixtures"))]
    {
        None
    }
}

/// What `doze <args> --print` says it would run as root, run in `dir` as the person (it changes nothing that needs
/// root). Empty when it says nothing.
fn printed(cli: &Cli, dir: &Path, args: &[String]) -> Option<String> {
    let print = cli.print_args(args)?;
    let out = Command::new(&cli.program)
        .args(&print)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env_remove("DOZE_APP_CHILD")
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text.trim_end().to_owned())
}

impl AdminView {
    pub(crate) fn new(
        job: Job,
        (hub, facts, fixture): (Hub, Entity<Facts>, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AdminView {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.observe(&facts, |v: &mut AdminView, _, cx| v.doctor_answered(cx))
            .detach();
        let cli = if fixture {
            Some(Cli::go(PathBuf::from("doze")))
        } else {
            Cli::from_env()
        };
        let mut v = AdminView {
            job,
            cli,
            hub,
            facts,
            fixture,
            steps: Vec::new(),
            phase: Phase::Planning,
            lines: Vec::new(),
            waiting: None,
            seen: plan::Seen::default(),
            scroll: ScrollHandle::new(),
            focus,
        };
        v.plan(cx);
        v
    }

    /// Works out the steps (and asks the CLI for each root script) off the UI thread.
    fn plan(&mut self, cx: &mut Context<Self>) {
        let Some(cli) = self.cli.clone() else {
            self.phase = Phase::Done(
                "this app cannot find doze to run the setup: copy the fix into a terminal".into(),
                true,
            );
            return;
        };
        let (job, home, stacks, fixture) = (
            self.job.clone(),
            self.hub.home().to_path_buf(),
            self.hub.stacks(),
            self.fixture,
        );
        let admin = fixture || doze_os::privilege::admin_supported();
        let work = cx.background_executor().spawn(async move {
            let (planned, seen) = job.plan(&cli, &home, &stacks, admin);
            let shown = planned
                .into_iter()
                .map(|step| {
                    let root = match &step {
                        Planned::Doze { dir, args, .. } if !fixture => {
                            printed(&cli, dir.as_deref().unwrap_or(&home), args)
                        }
                        _ => None,
                    };
                    Shown {
                        asks: step.asks(&cli),
                        step,
                        root,
                    }
                })
                .collect::<Vec<_>>();
            (shown, seen)
        });
        cx.spawn(async move |this, cx| {
            let (steps, seen) = work.await;
            let _ = this.update(cx, |v, cx| {
                if v.phase != Phase::Planning {
                    return;
                }
                v.steps = steps;
                v.seen = seen;
                v.phase = if v.steps.is_empty() {
                    Phase::Done(v.job.nothing().into(), false)
                } else {
                    Phase::Ready
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// How many password prompts Run will raise.
    pub(crate) fn prompts(&self) -> usize {
        self.steps.iter().filter(|s| s.asks).count()
    }

    /// Run: the steps in order, what they print said as it comes.
    pub(crate) fn run(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::Ready {
            return;
        }
        if self.fixture {
            self.finish(
                Ended {
                    said: "a fixture runs nothing on this machine".into(),
                    bad: true,
                    outcome: None,
                },
                cx,
            );
            return;
        }
        let Some(cli) = self.cli.clone() else { return };
        self.phase = Phase::Running;
        cx.emit(AdminEvent::Busy(true));
        cx.notify();
        let (tx, rx) = async_channel::unbounded::<Said>();
        let mut hands = SystemHands::new(cli.clone(), self.hub.home().to_path_buf());
        hands.out = Some(std::sync::Arc::new(move |s| {
            let _ = tx.send_blocking(s);
        }));
        hands.admin_runner = admin_runner();
        let (job, home, stacks, seen) = (
            self.job.clone(),
            self.hub.home().to_path_buf(),
            self.hub.stacks(),
            self.seen.clone(),
        );
        let admin = doze_os::privilege::admin_supported() || hands.admin_runner.is_some();
        let work = cx.background_executor().spawn(async move {
            plan::run(&job, &cli, &mut hands, (&home, &stacks, admin), &seen)
        });
        cx.spawn(async move |this, cx| {
            while let Ok(line) = rx.recv().await {
                crate::outward::note_line(&line);
                let alive = this.update(cx, |v, cx| {
                    v.lines.push(line);
                    v.scroll.scroll_to_bottom();
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
            let ended = work.await;
            let _ = this.update(cx, |v, cx| v.finish(ended, cx));
        })
        .detach();
    }

    /// The job ended: doctor and the leftovers look again; a repair's words wait for doctor.
    fn finish(&mut self, ended: Ended, cx: &mut Context<Self>) {
        cx.emit(AdminEvent::Busy(false));
        let stacks = self.hub.stacks();
        let seen = self.facts.update(cx, |f, cx| {
            f.forget_doctor(cx);
            f.forget_leftovers();
            let _ = f.doctor(&stacks, cx);
            f.fresh_after()
        });
        // With no stack folder doctor cannot run (it reads the names setting from a config): nothing to wait for.
        let doctor_can = crate::facts::doctor_dir(&stacks).is_some();
        match ended.outcome {
            Some(Outcome::Done) if !self.fixture && doctor_can => {
                self.waiting = Some((Outcome::Done, seen));
                self.phase = Phase::Checking;
            }
            Some(Outcome::Done) if !self.fixture => self.done("every step ran".into(), false, cx),
            _ => self.done(ended.said, ended.bad, cx),
        }
        cx.notify();
    }

    fn done(&mut self, said: String, bad: bool, cx: &mut Context<Self>) {
        crate::outward::note_line(&Said::Note(format!("done {said}")));
        self.phase = Phase::Done(said.clone(), bad);
        cx.emit(AdminEvent::Say(said, bad));
        cx.notify();
    }

    /// Doctor answered: a repair says what it came to, against the new report.
    fn doctor_answered(&mut self, cx: &mut Context<Self>) {
        let Some((out, seen)) = self.waiting.clone() else {
            return;
        };
        let stacks = self.hub.stacks();
        if self.facts.read(cx).doctor_seen() <= seen {
            // An answer from a run started before the repair ended: ask again (a run under way is not doubled).
            let _ = self.facts.update(cx, |f, cx| f.doctor(&stacks, cx));
            return;
        }
        let Some(d) = self.facts.update(cx, |f, cx| f.doctor(&stacks, cx)) else {
            return;
        };
        self.waiting = None;
        let at = crate::settings::doze_at(self.fixture);
        let (said, bad) = out.words(&parts(&d, &at, crate::VERSION));
        self.done(said, bad, cx);
    }

    /// Esc or Close: the sheet goes; a running job goes on and says how it ended.
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(AdminEvent::Close);
    }
}

/// A shot's sheet: the steps the job plans (a fixture's: no CLI asked), and, when `ran`, a run that printed and ended.
#[cfg(feature = "capture")]
pub(crate) fn stage(v: &mut AdminView, ran: bool, cx: &mut Context<AdminView>) {
    let cli = Cli::go(PathBuf::from("doze"));
    let home = v.hub.home().to_path_buf();
    v.steps = v
        .job
        .plan(&cli, &home, &[], true)
        .0
        .into_iter()
        .map(|step| Shown {
            asks: step.asks(&cli),
            root: matches!(step, Planned::Doze { .. }).then(|| {
                "mkdir -p /etc/resolver\nprintf 'nameserver 127.0.0.1\\nport 5323\\n' > /etc/resolver/doze".into()
            }),
            step,
        })
        .collect();
    v.phase = Phase::Ready;
    if ran {
        v.lines = vec![
            Said::Run("doze dns-setup".into()),
            Said::Line("› setting up .doze names (one admin prompt)".into()),
            Said::Note("waiting for the administrator's password…".into()),
            Said::Line("✓ resolver   /etc/resolver/doze → 127.0.0.1:5323".into()),
            Said::Line("✓ network    198.19.0.0/17 served by the doze network daemon".into()),
            Said::Run("doze trust".into()),
            Said::Line("✓ https://dev.shop.example  once the stack is up".into()),
        ];
        v.phase = Phase::Done("this machine is set up".into(), false);
    }
    cx.notify();
}
