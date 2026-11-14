//! Which files a load reads, parsed: doze.hcl and its sibling `*.doze.hcl`, or a folder's `*.hcl`
//! (Go's `gatherConfigFiles` and the parse loop of `loadFiles`).

use crate::error::{Error, Result, Sources, diag_error};
use crate::host::Host;
use crate::version;
use doze_hcl::diag::{Diagnostic, Diagnostics, Severity};
use doze_hcl::syntax::{Source, parse_body};
use hcl_edit::structure::Body;
use std::path::{Path, PathBuf};

/// One parsed file. `name` is the path as the load was given it (diagnostics print it).
#[derive(Debug)]
pub struct File {
    pub name: String,
    pub src: Source,
    pub body: Body,
}

/// Every file of a load, in merge order.
#[derive(Debug, Default)]
pub struct Files {
    pub files: Vec<File>,
}

impl Sources for Files {
    fn source(&self, file: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|f| f.name == file)
            .map(|f| f.src.text())
    }
}

impl Files {
    /// Parses `text` as the file `name`. A syntax error is refused as Go refuses it.
    pub fn push(&mut self, name: &str, text: &str, current: &str) -> Result<()> {
        let src = Source::new(text);
        match parse_body(&src) {
            Ok(body) => {
                self.files.push(File {
                    name: name.to_owned(),
                    src,
                    body,
                });
                Ok(())
            }
            Err(ds) => {
                let ds = Diagnostics(ds.0.into_iter().map(|d| d.in_file(name)).collect());
                self.files.push(File {
                    name: name.to_owned(),
                    src,
                    body: Body::default(),
                });
                Err(newer_syntax(self, ds, current))
            }
        }
    }
}

/// Go's `newerSyntax`: a file that will not parse, but says it needs a newer doze, says that.
fn newer_syntax(files: &Files, ds: Diagnostics, current: &str) -> Error {
    for d in ds.iter() {
        let Some(name) = d.file.as_deref() else {
            continue;
        };
        let Some(text) = files.source(name) else {
            continue;
        };
        if let Some(need) = doze_line(text)
            && version::needs_newer(current, &need)
        {
            return Error::msg(format!(
                "{name}: this stack needs doze {need} or newer; this doze is {}, and cannot read the file. Update doze (brew upgrade doze, or in the app: Settings › About)",
                current.trim_start_matches('v')
            ));
        }
    }
    diag_error(files, ds)
}

/// The version in a `doze = ">= 1.2"` line, read without parsing (Go's `reDozeLine`).
fn doze_line(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("doze")?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start().strip_prefix('"')?;
        let rest = rest.trim_start();
        let rest = rest.strip_prefix(">=").unwrap_or(rest).trim_start();
        let n = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'.')
            .count();
        let (v, after) = rest.split_at(n);
        (v.as_bytes().first().is_some_and(u8::is_ascii_digit)
            && after.trim_start().starts_with('"'))
        .then(|| v.to_owned())
    })
}

/// The files to merge and the primary path (for diagnostics and the project slug).
pub fn gather(host: &dyn Host, path: &str) -> Result<(Vec<String>, String)> {
    let abs = host.abs(Path::new(path));
    let meta = std::fs::metadata(&abs)
        .map_err(|e| Error::failed(format!("stat {path}: {}", io_text(&e))))?;
    if meta.is_dir() {
        let files = glob(host, Path::new(path), |n| n.ends_with(".hcl"));
        if files.is_empty() {
            return Err(Error::failed(format!("no .hcl files found in {path}")));
        }
        return Ok((files, path.to_owned()));
    }
    let dir = Path::new(path).parent().unwrap_or(Path::new(""));
    let mut files = vec![path.to_owned()];
    let own = crate::paths::clean(Path::new(path));
    for f in glob(host, dir, |n| n.ends_with(".doze.hcl")) {
        if Path::new(&f) != own {
            files.push(f);
        }
    }
    Ok((files, path.to_owned()))
}

/// Go's `filepath.Glob(filepath.Join(dir, "*<suffix>"))`, sorted, as paths joined to `dir`.
fn glob(host: &dyn Host, dir: &Path, want: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(host.abs(dir)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| want(n))
        .map(|n| path_string(&crate::paths::clean(&dir.join(n))))
        .collect();
    out.sort();
    out
}

pub fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Go's `*PathError` text for the common cases.
pub fn io_text(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        std::io::ErrorKind::IsADirectory => "is a directory".into(),
        _ => e.to_string(),
    }
}

/// Reads and parses every file. A file that cannot be read, or is not UTF-8, is refused with
/// hcl's diagnostics, as Go's parser refuses it.
pub fn read_all(host: &dyn Host, names: &[String], current: &str) -> Result<Files> {
    let mut files = Files::default();
    for name in names {
        let abs: PathBuf = host.abs(Path::new(name));
        let Ok(bytes) = std::fs::read(&abs) else {
            let d = Diagnostic {
                severity: Severity::Error,
                summary: "Failed to read file".into(),
                detail: format!(
                    "The configuration file {} could not be read.",
                    doze_hcl::value::quote::go_quote(name)
                ),
                subject: None,
                context: None,
                file: None,
            };
            return Err(diag_error(&files, d.into()));
        };
        match String::from_utf8(bytes) {
            Ok(text) => files.push(name, &text, current)?,
            Err(e) => {
                let ds = doze_hcl::check::check(e.as_bytes(), name);
                let ds = Diagnostics(ds.0.into_iter().map(|d| d.in_file(name)).collect());
                files.files.push(File {
                    name: name.clone(),
                    src: Source::new(&String::from_utf8_lossy(e.as_bytes())),
                    body: Body::default(),
                });
                return Err(newer_syntax(&files, ds, current));
            }
        }
    }
    Ok(files)
}

/// Go's `os.ReadFile` of an auto-vars file: its error is the load's, as text.
pub fn read_auto(host: &dyn Host, name: &str) -> Result<Files> {
    std::fs::read(host.abs(Path::new(name)))
        .map_err(|e| Error::msg(format!("read {name}: {}", io_text(&e))))?;
    read_all(host, &[name.to_owned()], "")
}
