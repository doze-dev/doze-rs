//! Finding the project: like git, from anywhere inside it. The default `-c doze.hcl` searches this directory and its
//! parents; an explicit `-c` means exactly that path. A directory given to `-c` is a project whose `*.hcl` files are
//! read together (Go's `-c <dir>`), with or without a `doze.hcl` among them.

use std::path::{Path, PathBuf};

/// The config's conventional name.
pub const CONFIG: &str = "doze.hcl";

/// A project on disk: its `doze.hcl` and the directory it lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub config: PathBuf,
    pub dir: PathBuf,
    /// The directory `-c` named, when it named one: every `*.hcl` in it is the config.
    pub source: Option<PathBuf>,
}

impl Project {
    /// The `doze.lock` beside the config.
    pub fn lock(&self) -> PathBuf {
        self.dir.join(crate::lock::FILE)
    }
}

/// Finds the project for `flag` (the `-c` value; `None` when not given) from `cwd`.
pub fn find(cwd: &Path, flag: Option<&Path>) -> Option<Project> {
    match flag {
        Some(path) => {
            let path = cwd.join(path);
            if path.is_dir() {
                return has_hcl(&path).then(|| Project {
                    config: path.join(CONFIG),
                    dir: path.clone(),
                    source: Some(path),
                });
            }
            let dir = path.parent()?.to_path_buf();
            path.is_file().then_some(Project {
                config: path,
                dir,
                source: None,
            })
        }
        None => cwd.ancestors().find_map(|dir| {
            let config = dir.join(CONFIG);
            config.is_file().then(|| Project {
                config,
                dir: dir.to_path_buf(),
                source: None,
            })
        }),
    }
}

/// Whether a directory holds a config file (`*.hcl`) to read.
fn has_hcl(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "hcl") && e.path().is_file())
    })
}
