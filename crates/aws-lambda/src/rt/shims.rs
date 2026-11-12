//! The Runtime API clients doze-aws ships for the runtimes AWS provides as managed images (Python, Node and Ruby). A host has the
//! interpreter and not `awslambdaric`, so these small clients stand in for it: they poll the Runtime API, resolve the handler, build
//! the context, and post the answer. They are embedded in the binary and written under `<data>/shims` the first time a function
//! needs one; `LAMBDA_RUNTIME_DIR` points there.

use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    ("bootstrap.py", include_str!("../shims/bootstrap.py")),
    ("bootstrap.mjs", include_str!("../shims/bootstrap.mjs")),
    ("bootstrap.rb", include_str!("../shims/bootstrap.rb")),
];

/// Where the clients are.
pub(crate) fn dir(base: &Path) -> PathBuf {
    base.join("shims")
}

/// Writes the clients that are missing or are not the ones in this binary.
///
/// # Errors
/// The disk refused.
pub(crate) fn ensure(base: &Path) -> Result<(), String> {
    let dir = dir(base);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, text) in FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|have| have == *text) {
            continue;
        }
        // Written to a temporary name and renamed, so a process starting now never reads half a file.
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
