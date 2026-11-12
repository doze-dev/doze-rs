//! Finding a program on `PATH`, the way the platform's shell would, without running a shell (PLAN.md D14).
//!
//! Unix: the first `dir/name` that is a file with an execute bit. Windows: if `name` has no extension, each
//! extension of `PATHEXT` in order (`.COM;.EXE;.BAT;.CMD` when unset), then the bare name only if it has one;
//! std's `Command` runs a found `.cmd`/`.bat` through its batch-file handling, arguments intact (W0).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Finds `name` on this process's `PATH` (and `PATHEXT` on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let pathext = std::env::var_os("PATHEXT");
    which_in(name, &path, pathext.as_deref())
}

/// Finds `name` on the given `PATH` value; `pathext` is used on Windows only.
pub fn which_in(name: &str, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    // A name with a separator is a path, not a search.
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        let p = PathBuf::from(name);
        return is_runnable(&p).then_some(p);
    }
    let candidates = candidates(name, pathext);
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| {
            candidates
                .iter()
                .map(move |c| dir.join(c))
                .collect::<Vec<_>>()
        })
        .find(|p| is_runnable(p))
}

fn candidates(name: &str, pathext: Option<&OsStr>) -> Vec<OsString> {
    if !cfg!(windows) {
        return vec![OsString::from(name)];
    }
    let has_ext = Path::new(name).extension().is_some();
    let exts = pathext
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    let mut out: Vec<OsString> = Vec::new();
    if has_ext {
        out.push(OsString::from(name));
    }
    out.extend(
        exts.split(';')
            .filter(|e| !e.is_empty())
            .map(|e| OsString::from(format!("{name}{}", e.to_ascii_lowercase()))),
    );
    out
}

#[cfg(unix)]
fn is_runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_runnable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_with_a_separator_is_not_searched() {
        assert_eq!(which_in("/no/such/tool", OsStr::new("/bin"), None), None);
    }

    #[test]
    fn windows_tries_pathext_in_order_and_the_bare_name_only_with_an_extension() {
        if cfg!(windows) {
            let c = candidates("psql", Some(OsStr::new(".EXE;.CMD")));
            assert_eq!(
                c,
                vec![OsString::from("psql.exe"), OsString::from("psql.cmd")]
            );
            let c = candidates("run.cmd", Some(OsStr::new(".EXE")));
            assert_eq!(
                c,
                vec![OsString::from("run.cmd"), OsString::from("run.cmd.exe")]
            );
        } else {
            assert_eq!(
                candidates("psql", Some(OsStr::new(".EXE"))),
                vec![OsString::from("psql")]
            );
        }
    }
}
