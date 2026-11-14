#![no_main]
//! Any command line (arguments split at NUL), run in-process with no project, no daemon and no network: doze ends
//! with an exit class and never panics, whatever clap and the commands are handed.

use doze_cli::{Ctx, Host, Terminals, registry};
use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

struct Nothing;

impl Host for Nothing {
    fn var(&self, _: &str) -> Option<String> {
        None
    }
    fn open_url(&self, _: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn spawn_background(&self, _: &[&str]) -> std::io::Result<()> {
        Ok(())
    }
    fn exe(&self) -> Option<PathBuf> {
        None
    }
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let argv = std::iter::once("doze").chain(text.split('\0')).map(Into::into).collect();
    let nowhere = std::env::temp_dir().join("doze-fuzz-argv-nowhere");
    let Some(key) = registry::signed::builtin_key() else { return };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ctx = Ctx {
        host: &Nothing,
        out: &mut out,
        err: &mut err,
        term: Terminals::default(),
        home: nowhere.join("home"),
        cwd: nowhere.join("cwd"),
        registry: registry::Config { bases: vec![registry::Base::Dir(nowhere.join("reg"))], key, budget: std::time::Duration::from_millis(10) },
        now: std::time::SystemTime::UNIX_EPOCH,
    };
    let _ = doze_cli::run(argv, &mut ctx).code();
});
