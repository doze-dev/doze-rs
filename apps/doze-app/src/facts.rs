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

/// How old the machine's report may be before it is asked again (Go's `doctorCached`).
const DOCTOR_AGE: Duration = Duration::from_secs(30);

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
    stamp: Option<SystemTime>,
    shape: Shape,
}

/// The facts, shared by the pages.
pub(crate) struct Facts {
    mode: Mode,
    doctor: Option<(Instant, Doctor)>,
    doctor_asked: bool,
    shapes: HashMap<PathBuf, Known>,
    asking: HashSet<PathBuf>,
    /// Each stack's domain setup, by its key and domains (`names`).
    trust: HashMap<String, (Instant, Vec<doze_desk::machine_names::Step>)>,
    trust_asked: HashSet<String>,
    /// Who holds the front door, and when that was read.
    front: Option<(Instant, Option<String>)>,
    /// The registry's URL for each host asked about, and when.
    urls: HashMap<String, (Instant, Option<String>)>,
    /// What stacks left behind, as last counted, and whether a count is under way.
    left: Option<(Instant, doze_desk::leftovers::Counts)>,
    left_asked: bool,
}

/// The `doze` a terminal would run. The app started from Finder has launchd's short PATH, so the places a terminal's
/// PATH has are looked at too (Go `dozeOnPath`).
pub(crate) fn find_doze() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(Path::new(&h).join(".local/bin"));
        dirs.push(Path::new(&h).join("go/bin"));
    }
    dirs.into_iter()
        .map(|d| d.join("doze"))
        .find(|p| p.is_file())
}

fn stamp(dir: &Path) -> Option<SystemTime> {
    std::fs::metadata(dir.join("doze.hcl"))
        .and_then(|m| m.modified())
        .ok()
}

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
            shapes: HashMap::new(),
            asking: HashSet::new(),
            trust: HashMap::new(),
            trust_asked: HashSet::new(),
            front: None,
            urls: HashMap::new(),
            left: None,
            left_asked: false,
        }
    }

    /// The shape of the stopped stack in `dir`, as far as it is known; asks the CLI when it is not, or when the file
    /// changed since.
    pub(crate) fn shape(&mut self, dir: &str, cx: &mut Context<Self>) -> Option<Shape> {
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() {
            return None;
        }
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

    fn ask_status(
        &mut self,
        doze: PathBuf,
        dir: PathBuf,
        at: Option<SystemTime>,
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
            .is_some_and(|(at, _)| at.elapsed() < DOCTOR_AGE);
        // doctor reads the machine's setup from inside a stack's folder (outside one it has no config to read the
        // names setting from); the first stack running, else the first one at all.
        let dir = stacks
            .iter()
            .find(|s| s.running && Path::new(&s.dir).is_absolute())
            .or_else(|| {
                stacks
                    .iter()
                    .find(|s| Path::new(&s.dir).join("doze.hcl").is_file())
            })
            .map(|s| PathBuf::from(&s.dir));
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
