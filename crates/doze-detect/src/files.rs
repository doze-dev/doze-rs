//! A compose file's services, a Makefile's preparing targets, `.env`, and the small helpers the readers share.

use crate::{Found, Scan, Task, services};
use std::path::{Path, PathBuf};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

/// The Makefile targets that read as preparing something.
const TASK_TARGETS: &[&str] = &[
    "migrate",
    "migrate-up",
    "db-migrate",
    "migrations",
    "seed",
    "db-seed",
    "setup",
    "bootstrap",
    "generate",
    "codegen",
];

/// The largest compose file or Makefile read.
const MAX_FILE: u64 = 1 << 20;

/// A folder's entries by name, `(name, path)`; nothing when it cannot be read.
pub(crate) fn read_dir_sorted(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub(crate) fn exists(dir: &Path, name: &str) -> bool {
    dir.join(name).exists()
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= MAX_FILE)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}

/// A name as a block label: lower case, every other character a dash, no dash at either end (Go's `slug`).
pub fn slug(s: &str) -> String {
    let out: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    out.trim_matches('-').to_owned()
}

/// An image without its tag (`postgres:16` is `postgres`; a registry port stays).
pub(crate) fn image_base(image: &str) -> &str {
    match (image.rfind(':'), image.rfind('/')) {
        (Some(c), Some(s)) if c > s => image.get(..c).unwrap_or(image),
        (Some(c), None) => image.get(..c).unwrap_or(image),
        _ => image,
    }
}

/// The container side of a compose port (`"8080:80/tcp"` is 80); 0 when it is not a number.
fn container_port(spec: &str) -> u16 {
    let last = spec.rsplit(':').next().unwrap_or(spec);
    let n = last.split('/').next().unwrap_or(last);
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    n.parse().unwrap_or(0)
}

/// A compose service that builds the project's own code, which is an app, not something to run beside it.
fn looks_like_build(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "app" | "api" | "web" | "worker"
    )
}

/// One compose service: its name, its image, its first port.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposeService {
    pub name: String,
    pub image: String,
    pub port: String,
}

/// The services of a compose file, by name. Read from the parser's events, so anchors and aliases are never
/// expanded (an alias bomb costs nothing); `None` when the text is not YAML.
// fuzz:parser fuzz_project_files
pub fn compose_services(text: &str) -> Option<Vec<ComposeService>> {
    let mut r = ComposeReader::default();
    Parser::new_from_str(text).load(&mut r, false).ok()?;
    let mut out = r.services;
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    Some(out)
}

#[derive(Debug)]
enum Frame {
    Map { key: Option<String> },
    Seq { index: usize },
}

#[derive(Default)]
struct ComposeReader {
    stack: Vec<Frame>,
    services: Vec<ComposeService>,
}

impl ComposeReader {
    /// The keys (and sequence positions, as `#n`) leading to the value about to be read.
    fn path(&self) -> Vec<String> {
        self.stack
            .iter()
            .map(|f| match f {
                Frame::Map { key } => key.clone().unwrap_or_default(),
                Frame::Seq { index } => format!("#{index}"),
            })
            .collect()
    }

    fn service(&mut self, name: &str) -> Option<&mut ComposeService> {
        if !self.services.last().is_some_and(|s| s.name == name) {
            self.services.push(ComposeService {
                name: name.to_owned(),
                ..ComposeService::default()
            });
        }
        self.services.last_mut()
    }

    /// A value was read in the innermost container: a map wants its next key, a list its next item.
    fn advance(&mut self) {
        match self.stack.last_mut() {
            Some(Frame::Map { key }) => *key = None,
            Some(Frame::Seq { index }) => *index += 1,
            None => {}
        }
    }

    fn value(&mut self, text: &str) {
        let path = self.path();
        match path.as_slice() {
            [s, name, k] if s == "services" && k == "image" => {
                if let Some(svc) = self.service(name) {
                    svc.image = text.to_owned();
                }
            }
            [s, name, k, first] if s == "services" && k == "ports" && first == "#0" => {
                if let Some(svc) = self.service(name) {
                    svc.port = text.to_owned();
                }
            }
            _ => {}
        }
        self.advance();
    }
}

impl MarkedEventReceiver for ComposeReader {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        match ev {
            Event::Scalar(text, ..) => {
                if let Some(Frame::Map { key }) = self.stack.last_mut()
                    && key.is_none()
                {
                    *key = Some(text);
                    return;
                }
                self.value(&text);
            }
            Event::Alias(_) => self.value(""),
            Event::MappingStart(..) => {
                if let [s, name] = self.path().as_slice()
                    && s == "services"
                {
                    let name = name.clone();
                    self.service(&name);
                }
                self.stack.push(Frame::Map { key: None });
            }
            Event::SequenceStart(..) => self.stack.push(Frame::Seq { index: 0 }),
            Event::MappingEnd | Event::SequenceEnd => {
                self.stack.pop();
                self.advance();
            }
            _ => {}
        }
    }
}

pub(crate) fn compose(s: &mut Scan<'_>, dir: &Path) {
    for file in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ] {
        let Some(list) = read_small(&dir.join(file)).and_then(|t| compose_services(&t)) else {
            continue;
        };
        for svc in list {
            compose_one(s, file, &svc);
        }
        break;
    }
}

fn compose_one(s: &mut Scan<'_>, file: &str, svc: &ComposeService) {
    let img = image_base(&svc.image);
    let why = format!("{file} has a {img} service");
    let mut matched = false;
    let declared = s.declared;
    for d in declared
        .iter()
        .filter(|d| d.detect.images.iter().any(|i| i == img))
    {
        s.add_module(d, why.clone());
        matched = true;
    }
    for r in services::RECIPES.iter().filter(|r| r.images.contains(&img)) {
        s.add_service(r.name, r.port, &svc.image, why.clone());
        matched = true;
    }
    if !matched && !svc.image.is_empty() && !looks_like_build(&svc.name) {
        s.add_service(
            &slug(&svc.name),
            container_port(&svc.port),
            &svc.image,
            format!("{file} has a {} service ({})", svc.name, svc.image),
        );
    }
}

/// The preparing targets a Makefile names, in the file's order.
// fuzz:parser fuzz_project_files
pub fn make_targets(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let mut chars = name.chars();
            let ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            let follows = rest.is_empty() || rest.starts_with(char::is_whitespace);
            (ok && follows).then(|| name.to_owned())
        })
        .collect()
}

pub(crate) fn makefile(dir: &Path) -> Vec<Task> {
    for file in ["Makefile", "makefile", "GNUmakefile"] {
        let Some(text) = read_small(&dir.join(file)) else {
            continue;
        };
        return make_targets(&text)
            .into_iter()
            .filter(|t| TASK_TARGETS.contains(&t.as_str()))
            .map(|t| Task {
                name: slug(&t),
                run: format!("make {t}"),
                manual: t.contains("seed"),
                why: format!("{file} has a {t} target"),
            })
            .collect();
    }
    Vec::new()
}

pub(crate) fn env_files(dir: &Path, found: &mut Found) {
    if exists(dir, ".env") {
        found.env_file = Some(".env".into());
    } else if exists(dir, ".env.example") {
        found
            .notes
            .push(".env.example lists variables; copy it to .env and the apps will read it".into());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn compose_services_by_name_with_image_and_first_port() {
        let t = "x-base: &base\n  image: never\nservices:\n  db:\n    image: postgres:16\n    ports: [\"5432:5432\"]\n  app:\n    build: .\n  web:\n    <<: *base\n    ports:\n      - 8080:80\n";
        let got = compose_services(t).unwrap();
        let names: Vec<_> = got
            .iter()
            .map(|s| (s.name.as_str(), s.image.as_str(), s.port.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("app", "", ""),
                ("db", "postgres:16", "5432:5432"),
                ("web", "", "8080:80")
            ]
        );
        assert_eq!(container_port("8080:80/tcp"), 80);
        assert_eq!(image_base("localhost:5000/x:1"), "localhost:5000/x");
    }

    #[test]
    fn a_rule_is_a_name_then_a_colon_then_space_or_nothing() {
        assert_eq!(
            make_targets("build:\n\tgo build\nmigrate: deps\nx := 1\n.PHONY: seed\nseed:\n"),
            ["build", "migrate", "seed"]
        );
    }

    #[test]
    fn slugs_are_go_s() {
        assert_eq!(slug("My_App 2!"), "my-app-2");
        assert_eq!(slug("--"), "");
    }
}
