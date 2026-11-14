//! Where a stack keeps its state (Go's `paths.go`) and the `.doze` names its blocks get
//! (`DomainFor`, from `domains.go`).

use crate::files::path_string;
use crate::host::Host;
use crate::model::Config;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// The suffix every doze name ends in.
pub const DOMAIN_SUFFIX: &str = "doze";

/// Go's `filepath.Clean`: `.` and `..` resolved lexically, no trailing slash.
pub fn clean(p: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) => {}
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().collect()
}

/// Go's `filepath.Dir`: the folder of `p`, `.` for a bare name.
pub fn dir_of(p: &str) -> String {
    match Path::new(p).parent() {
        Some(d) if !d.as_os_str().is_empty() => path_string(&clean(d)),
        Some(_) => ".".into(),
        None => path_string(&clean(Path::new(p))),
    }
}

/// The folder that holds the config: `path` itself if it is a folder, else its parent.
pub fn config_dir_of(host: &dyn Host, path: &str) -> String {
    if host.abs(Path::new(path)).is_dir() {
        return path.to_owned();
    }
    dir_of(path)
}

/// `<home>/projects/<slug>`'s slug: the config folder's name and a hash of its absolute path.
pub fn project_slug(host: &dyn Host, config_path: &str) -> String {
    if config_path.is_empty() {
        return "default".into();
    }
    let abs = host.abs(Path::new(config_path));
    let root = abs.parent().map_or_else(|| abs.clone(), Path::to_path_buf);
    let sum = Sha256::digest(path_string(&root).as_bytes());
    let base = root
        .file_name()
        .map_or_else(|| "/".into(), |n| n.to_string_lossy().into_owned());
    let hash = hex::encode(sum);
    format!("{}-{}", sanitize_slug(&base), hash.get(..8).unwrap_or(""))
}

fn sanitize_slug(s: &str) -> String {
    let mut b = String::new();
    let mut prev_dash = false;
    for r in s.to_lowercase().chars() {
        if r.is_ascii_lowercase() || r.is_ascii_digit() {
            b.push(r);
            prev_dash = false;
        } else if !prev_dash {
            b.push('-');
            prev_dash = true;
        }
    }
    let out = b.trim_matches('-');
    if out.is_empty() {
        "project".into()
    } else {
        out.into()
    }
}

/// Go's `resolveHome`: `home`, else `$DOZE_HOME`, else `~/.doze`; and the data folder under it.
pub fn resolve_home(cfg: &mut Config, host: &dyn Host) -> Result<(), String> {
    if cfg.home.is_empty() {
        cfg.home = host.env("DOZE_HOME").unwrap_or_default();
    }
    if cfg.home.is_empty() {
        let home = host.home_dir().ok_or_else(|| {
            "resolving home dir for default doze home: $HOME is not defined".to_owned()
        })?;
        cfg.home = path_string(&home.join(".doze"));
    }
    if cfg.data_dir.is_empty() {
        cfg.data_dir = path_string(
            &Path::new(&cfg.home)
                .join("projects")
                .join(project_slug(host, &cfg.path)),
        );
    }
    Ok(())
}

impl Config {
    /// Where a block keeps its data: `<data_dir>/clusters/<name>`.
    pub fn cluster_dir(&self, name: &str) -> String {
        path_string(&Path::new(&self.data_dir).join("clusters").join(name))
    }

    /// The stack's name as a DNS label: `stack`, else the config folder's name.
    pub fn stack(&self) -> String {
        let n = if self.stack_name.is_empty() {
            Path::new(&self.dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            self.stack_name.clone()
        };
        let l = domain_label(&n);
        if l.is_empty() { "default".into() } else { l }
    }

    /// `<name>.<stack>.doze`.
    pub fn domain_for(&self, name: &str) -> String {
        format!("{}.{}.{DOMAIN_SUFFIX}", domain_label(name), self.stack())
    }
}

/// A name as a DNS label: lower case, anything but letters and digits a dash, no dash at the ends.
pub fn domain_label(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|r| {
            if r.is_ascii_lowercase() || r.is_ascii_digit() {
                r
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').to_owned()
}
