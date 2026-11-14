//! What the pages need that no daemon says: a stopped stack's shape (its doze.hcl, read by `doze --json status` and by
//! the app itself) and the machine's setup (`doze --json doctor`). Both come from the CLI's machine contract
//! (docs/port/app/PLAN.md §4.1), run off the UI thread; a page shows what is known at once (the file read in place,
//! "checking…" for the machine) and redraws when the CLI has answered.
//!
//! A fixture source answers from its fixture instead (`status.json` beside a stopped stack's doze.hcl, and a machine
//! set up as the docs describe), so a screenshot is the same on every machine.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use doze_desk::StackView;
use doze_desk::library::{Check, Doctor, parse_doctor};
use doze_desk::overview::Shape;
use gpui::{AsyncApp, Context, WeakEntity};

mod left;
mod names;

/// How old the machine's report may be before it is asked again, in the background: half a minute while a window
/// shows a page, five minutes otherwise (Go `doctorWithin(stacks, lookEvery(30s))`; the Library's own read is 30 s, and
/// it reads only while it shows).
pub(crate) fn doctor_age(shown: bool) -> Duration {
    Duration::from_secs(if shown { 30 } else { 5 * 60 })
}

/// Whether the window shows a page (the hub hears so from its visibility); without the app's state (a shot), it does.
fn shown(cx: &gpui::App) -> bool {
    cx.try_global::<crate::app::State>()
        .is_none_or(|s| s.hub.visible())
}

/// Where the answers come from.
#[derive(Debug, Clone)]
pub(crate) enum Mode {
    /// The `doze` on this machine, if one is found.
    Live(Option<PathBuf>),
    /// A fixture's own answers.
    #[cfg_attr(not(feature = "fixtures"), allow(dead_code))]
    // WHY: only a build with fixtures makes a fixture source
    Fixture,
}

/// A stopped stack's shape, and the doze.hcl it was read from.
#[derive(Debug, Clone)]
struct Known {
    stamp: Option<Stamp>,
    shape: Shape,
}

/// The facts, shared by the pages.
pub(crate) struct Facts {
    mode: Mode,
    doctor: Option<(Instant, Doctor)>,
    doctor_asked: bool,
    /// How many answers doctor has given: a repair waits for the next.
    doctor_seen: u64,
    /// The two-second look at the configs is running.
    looking_configs: bool,
    shapes: HashMap<PathBuf, Known>,
    asking: HashSet<PathBuf>,
    /// Each stack's domain setup, by its key and domains (`names`).
    trust: HashMap<String, (Instant, Vec<doze_desk::machine_names::Step>)>,
    trust_asked: HashSet<String>,
    /// Who holds the front door, and when that was read.
    front: Option<(Instant, Option<String>)>,
    /// The registry's URL for each host asked about, and when.
    urls: HashMap<String, (Instant, Option<String>)>,
    /// What stacks left behind, as last found (`left`), whether a look is under way, and whether one was asked for.
    left: Option<(Instant, Vec<doze_desk::settings::Leftover>)>,
    left_asked: bool,
    left_wanted: bool,
}

/// The `doze` a terminal would run, from the command source (`doze_desk::settings::cli`, where `DOZE_APP_CLI` can
/// name another). The app started from Finder has launchd's short PATH, so the places a terminal's PATH has are
/// looked at too (Go `dozeOnPath`).
pub(crate) fn find_doze() -> Option<PathBuf> {
    doze_desk::settings::Cli::from_env().map(|c| c.program)
}

/// The folder doctor runs in: it reads the names setting from a stack's config, so the first stack running, else the
/// first with a doze.hcl.
pub(crate) fn doctor_dir(stacks: &[StackView]) -> Option<PathBuf> {
    stacks
        .iter()
        .find(|s| s.running && Path::new(&s.dir).is_absolute())
        .or_else(|| {
            stacks
                .iter()
                .find(|s| Path::new(&s.dir).join("doze.hcl").is_file())
        })
        .map(|s| PathBuf::from(&s.dir))
}

/// A stack's config as last seen: the newest change among its files, and their sizes together.
type Stamp = (SystemTime, u64);

/// The stamp of the stack's config in `dir` (Go `configStamp`): `doze.hcl`, every `*.doze.hcl` and every
/// `*.auto.doze.vars` beside it, the newest change and the sizes summed, so an edit to any of them is seen.
pub(crate) fn stamp(dir: &Path) -> Option<Stamp> {
    let main = std::fs::metadata(dir.join("doze.hcl")).ok()?;
    let mut out = (main.modified().ok()?, main.len());
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name != "doze.hcl"
            && (name.ends_with(".doze.hcl") || name.ends_with(".auto.doze.vars"))
            && let Ok(m) = e.metadata()
            && let Ok(t) = m.modified()
        {
            out = (out.0.max(t), out.1 + m.len());
        }
    }
    Some(out)
}

/// How often the configs the pages have read are looked at again (Go `configLook`).
const CONFIG_LOOK: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs `doze --json <args>` in `dir`, for its envelope. The app's own marker is not passed on.
pub(crate) fn run(doze: &Path, dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(doze)
        .arg("--json")
        .args(args)
        .current_dir(dir)
        .env_remove("DOZE_APP_CHILD")
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()
}

impl Facts {
    pub(crate) fn new(mode: Mode) -> Facts {
        Facts {
            mode,
            doctor: None,
            doctor_asked: false,
            doctor_seen: 0,
            looking_configs: false,
            shapes: HashMap::new(),
            asking: HashSet::new(),
            trust: HashMap::new(),
            trust_asked: HashSet::new(),
            front: None,
            urls: HashMap::new(),
            left: None,
            left_asked: false,
            left_wanted: false,
        }
    }

    /// The shape of the stopped stack in `dir`, as far as it is known; asks the CLI when it is not, or when the file
    /// changed since.
    pub(crate) fn shape(&mut self, dir: &str, cx: &mut Context<Self>) -> Option<Shape> {
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() {
            return None;
        }
        self.look_configs(cx);
        let now = stamp(&dir);
        if let Some(k) = self.shapes.get(&dir).filter(|k| k.stamp == now) {
            return Some(k.shape.clone());
        }
        match self.mode.clone() {
            Mode::Fixture => {
                let status = std::fs::read_to_string(dir.join("status.json")).ok();
                let shape = Shape::read(&dir, status.as_deref());
                self.shapes.insert(
                    dir,
                    Known {
                        stamp: now,
                        shape: shape.clone(),
                    },
                );
                Some(shape)
            }
            Mode::Live(doze) => {
                // The file alone, at once; the CLI's word when it comes.
                let shape = Shape::read(&dir, None);
                self.shapes.entry(dir.clone()).or_insert(Known {
                    stamp: None,
                    shape: shape.clone(),
                });
                if let Some(doze) = doze
                    && self.asking.insert(dir.clone())
                {
                    self.ask_status(doze, dir, now, cx);
                }
                Some(shape)
            }
        }
    }

    /// Every two seconds while shapes are known, their configs are stamped again; a change wakes the pages (which
    /// then read the file again), and nothing else happens: an unchanged look redraws nothing.
    fn look_configs(&mut self, cx: &mut Context<Self>) {
        if self.looking_configs || self.shapes.is_empty() {
            return;
        }
        self.looking_configs = true;
        cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(CONFIG_LOOK).await;
                let go_on = this.update(cx, |f, cx| {
                    let changed = f
                        .shapes
                        .iter()
                        .any(|(dir, k)| k.stamp.is_some() && stamp(dir) != k.stamp);
                    if changed {
                        cx.notify();
                    }
                    if f.shapes.is_empty() {
                        f.looking_configs = false;
                    }
                    f.looking_configs
                });
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn ask_status(
        &mut self,
        doze: PathBuf,
        dir: PathBuf,
        at: Option<Stamp>,
        cx: &mut Context<Self>,
    ) {
        let job = cx.background_executor().spawn({
            let dir = dir.clone();
            async move {
                let json = run(&doze, &dir, &["status"]);
                Shape::read(&dir, json.as_deref())
            }
        });
        cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
            let shape = job.await;
            let _ = this.update(cx, |f, cx| {
                f.asking.remove(&dir);
                f.shapes.insert(dir, Known { stamp: at, shape });
                cx.notify();
            });
        })
        .detach();
    }

    /// Makes the machine's last answer old, so the next look asks doctor again (after a repair); what it said stays
    /// on screen until the new answer comes.
    pub(crate) fn forget_doctor(&mut self, cx: &mut Context<Self>) {
        if let Some((at, _)) = &mut self.doctor {
            *at = at.checked_sub(doctor_age(false)).unwrap_or(*at);
        }
        cx.notify();
    }

    /// Whether this is the machine (not a fixture's made-up one).
    pub(crate) fn live(&self) -> bool {
        matches!(self.mode, Mode::Live(_))
    }

    /// How many answers doctor has given so far.
    pub(crate) fn doctor_seen(&self) -> u64 {
        self.doctor_seen
    }

    /// The machine's checks; asks again when the last answer is old. `None` while the first answer is coming.
    pub(crate) fn doctor(
        &mut self,
        stacks: &[StackView],
        cx: &mut Context<Self>,
    ) -> Option<Doctor> {
        let doze = match &self.mode {
            Mode::Fixture => {
                let domains: Vec<&str> = stacks
                    .iter()
                    .flat_map(|s| s.instances.iter())
                    .flat_map(|v| v.domains.iter().map(String::as_str))
                    .collect();
                return Some(set_up_machine(&domains));
            }
            Mode::Live(doze) => doze.clone(),
        };
        let fresh = self
            .doctor
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < doctor_age(shown(cx)));
        let dir = doctor_dir(stacks);
        if let (false, false, Some(doze), Some(dir)) = (fresh, self.doctor_asked, doze, dir) {
            self.doctor_asked = true;
            let job = cx
                .background_executor()
                .spawn(async move { run(&doze, &dir, &["doctor"]) });
            cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
                let json = job.await;
                let _ = this.update(cx, |f, cx| {
                    f.doctor_asked = false;
                    let mut d = json.as_deref().and_then(parse_doctor).unwrap_or_default();
                    if doze_desk::library::zone_off() {
                        doze_desk::library::names_off(&mut d);
                    }
                    f.doctor = Some((Instant::now(), d));
                    f.doctor_seen += 1;
                    cx.notify();
                });
            })
            .detach();
        }
        self.doctor.as_ref().map(|(_, d)| d.clone())
    }
}

/// A machine set up as the docs describe (Go devserve's `setUpMachine`), for fixtures: names on, each declared domain
/// routed, the authority trusted.
pub(crate) fn set_up_machine(domains: &[&str]) -> Doctor {
    let check = |section: &'static str, name: &str, text: &str| Check {
        name: name.to_owned(),
        ok: true,
        text: text.to_owned(),
        section,
    };
    let mut checks = vec![
        check(
            "names",
            "network",
            "198.19.0.0/17 served by the doze network daemon",
        ),
        check(
            "names",
            "resolver route",
            "/etc/resolver/doze → 127.0.0.1:5323",
        ),
    ];
    for d in domains {
        checks.push(check(
            "domains",
            &format!("route {d}"),
            &format!("/etc/resolver/{d} → 127.0.0.1:5323"),
        ));
    }
    if !domains.is_empty() {
        checks.push(check(
            "domains",
            "certificate authority",
            "trusted in the System keychain · doze local CA · expires 2036-11-01",
        ));
    }
    Doctor { checks }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn the_report_is_kept_half_a_minute_on_screen_and_five_minutes_off_it() {
        assert_eq!(super::doctor_age(true), Duration::from_secs(30));
        assert_eq!(super::doctor_age(false), Duration::from_secs(300));
    }
}
