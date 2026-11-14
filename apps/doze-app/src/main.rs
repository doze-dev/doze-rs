//! doze's desktop app: a native GPUI window over the doze daemon, and the menu-bar item (docs/port/app/PLAN.md).
//!
//! This is M0: the shell (activation, the one window and its frame, single instance with argument forwarding, the
//! menu-bar item, `doze://` links, folders dropped on the window), the theme (five palettes, dark and light, from
//! the Go app's tokens), the action table (keymap, menus, ⌘K, the help sheet and every key chip from one table), the
//! Map, and `shot` and `bench`. Every screen reads the doze-desk hub (`source`).
//!
//! Lint carve-outs, written down (docs/port/app/PLAN.md step 0.1): none in this crate. It keeps the workspace's lints,
//! including no `unsafe` (the menu-bar item and the Dock badge are `doze_os::ui`), no `unwrap`, no indexing, functions
//! under 70 lines and files under 450. Two workspace-level differences: it is not a default member (GPUI is a few
//! hundred crates and needs Metal), and the vendored gpui-pre-apple patch is the root manifest's `[patch]`.

mod act;
mod actions;
mod activity;
mod app;
mod around;
mod assets;
mod bench;
mod cli;
mod console;
#[cfg(feature = "fixtures")]
mod driver;
mod empty;
mod facts;
#[cfg(feature = "fixtures")]
mod fixture;
mod handsoff;
mod instance;
mod kit;
mod library;
mod link;
#[cfg(feature = "fixtures")]
mod logbench;
mod logs;
mod map;
mod model;
mod motion;
mod open;
mod outward;
mod overlays;
mod overview;
mod prefs;
mod settings;
mod shell;
#[cfg(feature = "capture")]
mod shot;
#[cfg(feature = "capture")]
mod shot_act;
#[cfg(feature = "capture")]
mod shot_m3;
#[cfg(feature = "capture")]
mod shotover;
mod single;
mod source;
mod stderr_log;
mod theme;
mod tray;
mod widgets;

/// doze's version as the app shows it ("v0.1.2"): `DOZE_VERSION` when the release build sets it, else the crate's.
pub(crate) const VERSION: &str = match option_env!("DOZE_VERSION") {
    Some(v) => v,
    None => concat!("v", env!("CARGO_PKG_VERSION")),
};

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use source::Source;

/// The source a run reads: a fixture when one is named, else the doze home.
fn source_for(from: &cli::From, home: PathBuf) -> anyhow::Result<Source> {
    match (&from.fixture, from.synth) {
        (None, None) => Ok(Source::live(home)),
        #[cfg(feature = "fixtures")]
        (name, synth) => Source::fixture(name.as_deref().unwrap_or("synthetic"), synth),
        #[cfg(not(feature = "fixtures"))]
        _ => anyhow::bail!("--fixture and --synth need a build with the `fixtures` feature"),
    }
}

fn main() -> ExitCode {
    let launched = Instant::now();
    stderr_log::init();
    match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Cmd::App(a)) => app::run(a),
        Ok(cli::Cmd::Open(a)) => open::run(a),
        Ok(cli::Cmd::Bench(b)) => bench::run(b, launched),
        #[cfg(feature = "fixtures")]
        Ok(cli::Cmd::FixtureServe(name, root)) => fixture::serve(&name, root),
        #[cfg(not(feature = "fixtures"))]
        Ok(cli::Cmd::FixtureServe(..)) => {
            eprintln!("doze-app fixture-serve: build with --features fixtures");
            ExitCode::from(2)
        }
        #[cfg(feature = "capture")]
        Ok(cli::Cmd::Shot(s)) => shot::run(*s),
        #[cfg(not(feature = "capture"))]
        Ok(cli::Cmd::Shot(_)) => {
            eprintln!(
                "doze-app shot: this build has no offscreen capture (build with --features capture)"
            );
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!(
                "doze-app: {e}\n\nusage: doze-app [FOLDER | doze://…] [--show NAME] [--menu-bar] [--fixture NAME | --synth N]\n       doze-app shot --fixture NAME --screen library|tiles|list|map|grid|palette|help|instance|logs [--stack NAME] --theme T --mode dark|light --size WxH --out FILE\n                     [--select NAME] [--tab ID] [--filter TEXT] [--find-mode] [--level 0-3] [--hold PORT] [--density D]\n       doze-app bench [--fixture NAME | --synth N] [--idle SECONDS] [--display N] [--in-use] [--check [--budget MB]] [--logs [--follow SECONDS]]"
            );
            ExitCode::from(2)
        }
    }
}
