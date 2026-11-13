//! Acceptance on a real engine build (PLAN.md D12): a module's tests boot the engine doze-binaries publishes, found
//! through `DOZE_<ENGINE>_BINDIR`. A developer without the build sees the test say it skipped. The CI job a release's
//! `--accepted` comes from sets `DOZE_ACCEPTANCE`, and there a missing build fails: a skip must never read as a pass.

use crate::boot::Harness;
use std::path::PathBuf;

/// The variable that makes a missing engine build a failure.
pub const REQUIRED: &str = "DOZE_ACCEPTANCE";

/// A harness for `program` on the real `engine` build, `Ok(None)` (after saying so on stderr) if there is no build
/// and acceptance is not required, and an error if it is.
pub fn acceptance(program: &str, engine: &str) -> Result<Option<Harness>, String> {
    let var = format!("DOZE_{}_BINDIR", engine.to_ascii_uppercase());
    let Some(dir) = std::env::var_os(&var).filter(|d| !d.is_empty()) else {
        if std::env::var_os(REQUIRED).is_some() {
            return Err(format!(
                "{REQUIRED} is set but {var} is not: acceptance needs the real {engine} build"
            ));
        }
        eprintln!("skipped: set {var} to a directory holding the {engine} build to run acceptance");
        return Ok(None);
    };
    let bin_dir = PathBuf::from(dir);
    if !bin_dir.is_dir() {
        return Err(format!("{var}={} is not a directory", bin_dir.display()));
    }
    Ok(Some(Harness {
        program: program.into(),
        engine: engine.into(),
        bin_dir,
        full: "local".into(),
    }))
}
