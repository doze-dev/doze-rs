//! Finding the project: like git, from anywhere inside it. The default `-c doze.hcl` searches this directory and its
//! parents; an explicit `-c` means exactly that path (a directory stands for the `doze.hcl` in it).

use std::path::{Path, PathBuf};

/// The config's conventional name.
pub const CONFIG: &str = "doze.hcl";

/// A project on disk: its `doze.hcl` and the directory it lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub config: PathBuf,
    pub dir: PathBuf,
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
            let config = if path.is_dir() {
                path.join(CONFIG)
            } else {
                path
            };
            let dir = config.parent()?.to_path_buf();
            config.is_file().then_some(Project { config, dir })
        }
        None => cwd.ancestors().find_map(|dir| {
            let config = dir.join(CONFIG);
            config.is_file().then(|| Project {
                config,
                dir: dir.to_path_buf(),
            })
        }),
    }
}
