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

mod actions;
mod app;
mod assets;
mod bench;
mod cli;
mod facts;
#[cfg(feature = "fixtures")]
mod fixture;
mod kit;
mod library;
mod link;
mod map;
mod model;
mod overlays;
mod overview;
mod prefs;
mod shell;
#[cfg(feature = "capture")]
mod shot;
mod single;
mod source;
mod theme;
mod tray;
mod widgets;

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
    match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Cmd::App(a)) => app::run(a),
        Ok(cli::Cmd::Bench(b)) => bench::run(b, launched),
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
                "doze-app: {e}\n\nusage: doze-app [FOLDER | doze://…] [--show NAME] [--menu-bar] [--fixture NAME | --synth N]\n       doze-app shot --fixture NAME --screen library|tiles|list|map|grid|palette|help [--stack NAME] --theme T --mode dark|light --size WxH --out FILE\n       doze-app bench [--fixture NAME | --synth N] [--idle SECONDS]"
            );
            ExitCode::from(2)
        }
    }
}
