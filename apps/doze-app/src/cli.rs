//! The command line.
//!
//! ```text
//! doze-app [FOLDER | doze://…] [--show NAME] [--menu-bar]      the app (a second launch hands these to the first)
//! doze-app open [FOLDER | doze://…] [--show NAME]               hand over to the running app, or start it on its own
//!          [--fixture NAME | --synth N]                         a fixture stack instead of the doze home (dev builds)
//! doze-app shot --fixture larder --screen map --theme teal --mode light --size 1280x800 --out x.png
//!          [--select NAME] [--hover NAME] [--zoom Z] [--at X,Y] [--find TEXT] [--synth N] [--reduce-motion] [--wait MS]
//!          --screen instance --select api [--tab logs|console:ID|last run] [--hold PORT]
//!          --screen logs [--filter TEXT] [--find-mode] [--level 0-3]
//!          --screen map [--map lines,tile,trouble,hood:N,fly:P,region:NAME]
//!          --act sleep|restart|reset|reset-typed|stop|remove|action|action-typed|copy|toast-fail|recent|preflight|start
//!                |busy|failed|library-busy|verb-busy|empty|empty-found|add     (M2: done before the shot)
//! doze-app bench [--fixture NAME | --synth N] [--idle SECONDS]  first-frame time, then idle for a memory reading
//! doze-app bench --fixture shop --logs [--follow SECONDS]          the logs viewer: 100k lines scrolled, then followed
//! doze-app bench --fixture medium-150 --map                         the Map: frames while it pans, zooms and hovers
//! doze-app bench [--idle S] [--closed S]                         with no fixture: $DOZE_HOME (a fixture served apart)
//! doze-app fixture-serve NAME ROOT                               a fixture daemon alone, on its own socket under ROOT/home
//! doze-app status | logs | wake | …                              the doze CLI, whole (main.rs `as_cli`)
//! ```

use std::path::PathBuf;

use crate::theme::{Mode, Palette};

/// Where the stacks come from.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct From {
    pub(crate) fixture: Option<String>,
    pub(crate) synth: Option<usize>,
}

/// `doze-app [...]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct App {
    /// What a second launch would forward: everything but the source flags.
    pub(crate) forward: Vec<String>,
    pub(crate) from: From,
    /// Start in the menu bar only.
    pub(crate) menu_bar: bool,
}

/// Which screen a shot renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Screen {
    #[default]
    Map,
    Palette,
    Help,
    /// The Library, the first tab.
    Library,
    /// A stack's page in its Tiles, List or Grid view.
    Tiles,
    List,
    Grid,
    /// An instance's page: `--select NAME`, `--tab ID`.
    Instance,
    /// All logs.
    Logs,
    /// Activity: `--tab 1h|24h|7d`, `--select NAME` for one service alone, `--filter TEXT` for the find box.
    Activity,
    /// Settings, at a part.
    General,
    Machine,
    About,
}

/// `doze-app shot ...`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Shot {
    pub(crate) from: From,
    pub(crate) screen: Screen,
    pub(crate) palette: Palette,
    pub(crate) mode: Mode,
    pub(crate) size: (f32, f32),
    pub(crate) out: PathBuf,
    pub(crate) select: Option<String>,
    pub(crate) hover: Option<String>,
    pub(crate) zoom: Option<f32>,
    pub(crate) at: Option<(f64, f64)>,
    pub(crate) find: Option<String>,
    pub(crate) reduce: bool,
    /// The stack shown, by name (else the fixture's running one).
    pub(crate) stack: Option<String>,
    /// The rows' density: compact, cosy, roomy.
    pub(crate) density: Option<String>,
    /// The instance page's tab.
    pub(crate) tab: Option<String>,
    /// A port the shot holds while it renders (a port clash needs a real holder).
    pub(crate) hold: Option<u16>,
    /// The logs' filter, level and find mode.
    pub(crate) filter: Option<String>,
    pub(crate) level: Option<u8>,
    pub(crate) find_mode: bool,
    /// Something done before the shot (M2): a dialog asked for, a stack action started, a folder opened.
    pub(crate) act: Option<String>,
    /// ⌘K's box, its scope (`stack` or `stack/service`), and ⏎ on the row highlighted.
    pub(crate) query: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) enter: bool,
    /// Needs you's surface over the page: `bell`, `alerts`, `away`; or `note`, a notification said in the app (M3).
    pub(crate) over: Option<String>,
    /// Needs you sees this machine not set up (names and a domain), as Go's fixture server did.
    pub(crate) machine_unset: bool,
    /// How long to wait before the shot, in ms (`--wait`): for what only time shows, a daemon gone quiet.
    pub(crate) wait: u64,
    /// The Map's state for the shot, comma-separated: `lines`, `tile`, `trouble`, `hood:N` (on the selection),
    /// `fly:P` (find's flight to the selection, held at share P of the way), `region:NAME` (opened).
    pub(crate) map: Option<String>,
}

/// `doze-app bench ...`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Bench {
    pub(crate) from: From,
    pub(crate) idle: u64,
    /// How long it stays up after the window closes (`--closed`, default 10 s), for a reading of the closed app.
    pub(crate) closed: Option<u64>,
    /// The logs viewer under load instead (`--logs`), following for `follow` seconds.
    pub(crate) logs: bool,
    pub(crate) follow: u64,
    /// The Map under load instead (`--map`): frames drawn offscreen while it pans, zooms and hovers.
    pub(crate) map: bool,
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cmd {
    App(App),
    /// `doze-app open ...`: hand over to a running app, or start one on its own (`crate::open`).
    Open(App),
    Shot(Box<Shot>),
    Bench(Bench),
    /// `doze-app fixture-serve NAME ROOT`: a fixture daemon alone, on its stack's own socket under `ROOT/home`, so an app
    /// in another process can be measured without the fixture in it (feature `fixtures`).
    FixtureServe(String, std::path::PathBuf),
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn number<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse()
        .map_err(|_| format!("{flag}: `{s}` is not a number"))
}

/// Reads the arguments (without the program name).
pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Cmd, String> {
    let mut args = args.into_iter().peekable();
    match args.peek().map(String::as_str) {
        Some("shot") => {
            args.next();
            shot(args).map(|s| Cmd::Shot(Box::new(s)))
        }
        Some("bench") => {
            args.next();
            bench(args).map(Cmd::Bench)
        }
        Some("open") => {
            args.next();
            app(args).map(Cmd::Open)
        }
        Some("fixture-serve") => {
            args.next();
            let name = value(&mut args, "fixture-serve")?;
            let root = value(&mut args, "fixture-serve")?;
            Ok(Cmd::FixtureServe(name, root.into()))
        }
        _ => app(args).map(Cmd::App),
    }
}

/// A source flag, if `a` is one.
fn source(
    a: &str,
    args: &mut impl Iterator<Item = String>,
    from: &mut From,
) -> Result<bool, String> {
    match a {
        "--fixture" => from.fixture = Some(value(args, a)?),
        "--synth" => from.synth = Some(number(&value(args, a)?, a)?),
        _ => return Ok(false),
    }
    Ok(true)
}

fn app(mut args: impl Iterator<Item = String>) -> Result<App, String> {
    let mut out = App::default();
    while let Some(a) = args.next() {
        if source(&a, &mut args, &mut out.from)? {
            continue;
        }
        match a.as_str() {
            "--menu-bar" => out.menu_bar = true,
            "--show" => {
                out.forward.push(a);
                out.forward.push(value(&mut args, "--show")?);
            }
            s if s.starts_with('-') && !s.starts_with("--show=") => {
                return Err(format!("unknown flag {s}"));
            }
            _ => out.forward.push(a),
        }
    }
    Ok(out)
}

fn size(s: &str) -> Result<(f32, f32), String> {
    let (w, h) = s.split_once('x').ok_or("--size is WIDTHxHEIGHT")?;
    Ok((number(w, "--size")?, number(h, "--size")?))
}

/// A shot's flag that takes no value, if `a` is one.
fn switch(a: &str, s: &mut Shot) -> bool {
    match a {
        "--reduce-motion" => s.reduce = true,
        "--find-mode" => s.find_mode = true,
        "--machine-unset" => s.machine_unset = true,
        "--enter" => s.enter = true,
        _ => return false,
    }
    true
}

fn shot(mut args: impl Iterator<Item = String>) -> Result<Shot, String> {
    let mut s = Shot {
        size: (1280., 800.),
        out: PathBuf::from("shot.png"),
        mode: Mode::Dark,
        ..Shot::default()
    };
    while let Some(a) = args.next() {
        if source(&a, &mut args, &mut s.from)? {
            continue;
        }
        if switch(&a, &mut s) {
            continue;
        }
        let v = value(&mut args, &a)?;
        match a.as_str() {
            "--screen" => {
                s.screen = match v.as_str() {
                    "map" => Screen::Map,
                    "palette" => Screen::Palette,
                    "help" => Screen::Help,
                    "library" => Screen::Library,
                    "tiles" => Screen::Tiles,
                    "list" => Screen::List,
                    "grid" => Screen::Grid,
                    "instance" => Screen::Instance,
                    "logs" => Screen::Logs,
                    "activity" => Screen::Activity,
                    "general" | "settings" => Screen::General,
                    "machine" => Screen::Machine,
                    "about" => Screen::About,
                    _ => {
                        return Err(format!(
                            "--screen: no screen {v} (library, tiles, list, map, grid, palette, help, instance, logs, activity, general, machine, about)"
                        ));
                    }
                }
            }
            "--theme" => {
                s.palette = Palette::parse(&v).ok_or(format!("--theme: no palette {v}"))?
            }
            "--mode" => {
                s.mode =
                    Mode::parse(&v).ok_or(format!("--mode: {v} is not dark, light or system"))?
            }
            "--size" => s.size = size(&v)?,
            "--out" => s.out = PathBuf::from(v),
            "--select" => s.select = Some(v),
            "--hover" => s.hover = Some(v),
            "--zoom" => s.zoom = Some(number(&v, "--zoom")?),
            "--find" => s.find = Some(v),
            "--stack" => s.stack = Some(v),
            "--density" => s.density = Some(v),
            "--tab" => s.tab = Some(v),
            "--hold" => s.hold = Some(number(&v, "--hold")?),
            "--filter" => s.filter = Some(v),
            "--level" => s.level = Some(number(&v, "--level")?),
            "--act" => s.act = Some(v),
            "--query" => s.query = Some(v),
            "--scope" => s.scope = Some(v),
            "--over" => s.over = Some(v),
            "--wait" => s.wait = number(&v, "--wait")?,
            "--map" => s.map = Some(v),
            "--at" => {
                let (x, y) = v.split_once(',').ok_or("--at is X,Y")?;
                s.at = Some((number(x, "--at")?, number(y, "--at")?));
            }
            _ => return Err(format!("shot: unknown flag {a}")),
        }
    }
    Ok(s)
}

fn bench(mut args: impl Iterator<Item = String>) -> Result<Bench, String> {
    let mut b = Bench {
        idle: 20,
        follow: 30,
        ..Bench::default()
    };
    while let Some(a) = args.next() {
        if source(&a, &mut args, &mut b.from)? {
            continue;
        }
        match a.as_str() {
            "--idle" => b.idle = number(&value(&mut args, &a)?, "--idle")?,
            "--closed" => b.closed = Some(number(&value(&mut args, &a)?, "--closed")?),
            "--logs" => b.logs = true,
            "--map" => b.map = true,
            "--follow" => b.follow = number(&value(&mut args, &a)?, "--follow")?,
            _ => return Err(format!("bench: unknown flag {a}")),
        }
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::{Cmd, Screen, parse};
    use crate::theme::{Mode, Palette};

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_shot_reads_every_flag() {
        let Ok(Cmd::Shot(s)) = parse(args(
            "shot --fixture larder --screen help --theme teal --mode light --size 900x620 --out /tmp/x.png --select api --zoom 0.25 --at 10,20",
        )) else {
            panic!("a shot")
        };
        assert_eq!(s.from.fixture.as_deref(), Some("larder"));
        assert_eq!(
            (s.screen, s.palette, s.mode, s.size),
            (Screen::Help, Palette::Teal, Mode::Light, (900., 620.))
        );
        assert_eq!(
            (s.select.as_deref(), s.zoom, s.at),
            (Some("api"), Some(0.25), Some((10., 20.)))
        );
        assert!(parse(args("shot --screen nope")).is_err());
    }

    #[test]
    fn a_shot_inside_a_stack_and_the_logs_bench() {
        let Ok(Cmd::Shot(s)) = parse(args(
            "shot --fixture shop --screen instance --select api --tab logs --hold 8000 --filter x --find-mode --level 2",
        )) else {
            panic!("a shot")
        };
        assert_eq!(s.screen, Screen::Instance);
        assert_eq!(
            (s.tab.as_deref(), s.hold, s.filter.as_deref(), s.level),
            (Some("logs"), Some(8000), Some("x"), Some(2))
        );
        assert!(s.find_mode);
        let Ok(Cmd::Bench(b)) = parse(args("bench --fixture shop --logs --follow 5")) else {
            panic!("a bench")
        };
        assert!(b.logs && b.follow == 5);
    }

    #[test]
    fn the_app_forwards_folders_links_and_show() {
        let Ok(Cmd::App(a)) = parse(args("/code/shop --show db --synth 2000")) else {
            panic!("the app")
        };
        assert_eq!(a.forward, args("/code/shop --show db"));
        assert_eq!(a.from.synth, Some(2000));
        assert!(parse(args("--nope")).is_err());
    }
}
