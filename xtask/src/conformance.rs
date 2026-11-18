//! `cargo xtask conformance [--cmd TEMPLATE | --endpoint URL] [module ...]`: runs the pytest and boto3 harness in
//! `conformance/` against a server, one module at a time, and prints how many compared responses matched what
//! real AWS answered (the snapshots in `conformance/snapshots/`).
//!
//! `--cmd` starts a fresh server per module; `{addr}` and `{data}` in it are filled in. `--endpoint` uses a
//! server that is already running. `SERVER_CMD` is read when `--cmd` is not given. With no module names, every
//! module that has a recording runs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const DIR: &str = "conformance";

#[derive(Debug, Default, PartialEq, Eq)]
struct Score {
    exact: usize,
    differ: usize,
    unverified: usize,
    /// Differing labels whose only differing field is the error message text.
    wording_only: usize,
}

impl Score {
    fn total(&self) -> usize {
        self.exact + self.differ + self.unverified
    }
}

/// Where `--save` keeps each module's last score, one tab-separated line per module (the compatibility page reads it).
const RESULTS: &str = "conformance/results.tsv";

enum Target {
    Command(String),
    Endpoint(String),
}

pub(crate) fn run(args: Vec<String>) -> ExitCode {
    let save = args.iter().any(|a| a == "--save");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--save").collect();
    let (target, modules) = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "xtask conformance: {e}\nusage: conformance [--save] [--cmd TEMPLATE | --endpoint URL] [module ...]"
            );
            return ExitCode::from(2);
        }
    };
    let modules = if modules.is_empty() {
        recorded()
    } else {
        modules
    };
    println!(
        "{:<16} {:>6} {:>6} {:>8} {:>10} {:>11}",
        "module", "labels", "exact", "wording", "behaviour", "unverified"
    );
    let mut all = Score::default();
    let mut saved = Vec::new();
    for module in &modules {
        match score_module(&target, module) {
            Ok(s) => {
                saved.push((module.clone(), line_of(module, &s)));
                let behaviour = s.differ.saturating_sub(s.wording_only);
                println!(
                    "{module:<16} {:>6} {:>6} {:>8} {behaviour:>10} {:>11}",
                    s.total(),
                    s.exact,
                    s.wording_only,
                    s.unverified
                );
                all.exact += s.exact;
                all.differ += s.differ;
                all.unverified += s.unverified;
                all.wording_only += s.wording_only;
            }
            Err(e) => println!("{module:<16} {e}"),
        }
    }
    println!("total: {} labels, {}", all.total(), percent(&all));
    if save && let Err(e) = save_results(&saved) {
        eprintln!("xtask conformance: {RESULTS}: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// A module's saved line: module, labels, exact, wording, behaviour, unverified, the date it was scored.
fn line_of(module: &str, s: &Score) -> String {
    let date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    format!(
        "{module}\t{}\t{}\t{}\t{}\t{}\t{date}",
        s.total(),
        s.exact,
        s.wording_only,
        s.differ.saturating_sub(s.wording_only),
        s.unverified
    )
}

/// Replaces the lines of the modules just scored, keeps the others, and writes the file sorted by module.
fn save_results(scored: &[(String, String)]) -> std::io::Result<()> {
    let mut lines: BTreeMap<String, String> = fs::read_to_string(RESULTS)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|l| Some((l.split('\t').next()?.to_owned(), l.to_owned())))
        .collect();
    for (module, line) in scored {
        lines.insert(module.clone(), line.clone());
    }
    let mut out = String::from("# module\tlabels\texact\twording\tbehaviour\tunverified\tscored\n");
    for line in lines.values() {
        out.push_str(line);
        out.push('\n');
    }
    fs::write(RESULTS, out)
}

fn percent(s: &Score) -> String {
    let total = s.total().max(1);
    let pct = |n: usize| n * 100 / total;
    format!(
        "{}% exact, {}% ignoring wording",
        pct(s.exact),
        pct(s.exact + s.wording_only)
    )
}

fn parse_args(args: Vec<String>) -> Result<(Target, Vec<String>), String> {
    let mut target = None;
    let mut modules = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cmd" => target = Some(Target::Command(it.next().ok_or("--cmd needs a value")?)),
            "--endpoint" => {
                target = Some(Target::Endpoint(
                    it.next().ok_or("--endpoint needs a value")?,
                ))
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => modules.push(a),
        }
    }
    let target = match target {
        Some(t) => t,
        None => Target::Command(
            std::env::var("SERVER_CMD").map_err(|_| "give --cmd, --endpoint or SERVER_CMD")?,
        ),
    };
    Ok((target, modules))
}

/// Modules that have a recording: `snapshots/test_<module>.json`.
fn recorded() -> Vec<String> {
    let Ok(entries) = fs::read_dir(Path::new(DIR).join("snapshots")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| {
            n.strip_prefix("test_")?
                .strip_suffix(".json")
                .map(ToString::to_string)
        })
        .collect();
    out.sort();
    out
}

fn score_module(target: &Target, module: &str) -> Result<Score, String> {
    let (endpoint, server, data) = match target {
        Target::Endpoint(url) => (url.clone(), None, None),
        Target::Command(template) => {
            let (url, child, data) = start(template)?;
            (url, Some(child), Some(data))
        }
    };
    let text = pytest(module, &endpoint);
    if let Some(mut child) = server {
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(dir) = data {
        let _ = fs::remove_dir_all(dir);
    }
    text.map(|t| score(&t))
}

fn start(template: &str) -> Result<(String, Child, PathBuf), String> {
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let data = std::env::temp_dir().join(format!("xtask-conf-{}-{port}", std::process::id()));
    fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let line = template
        .replace("{addr}", &format!("127.0.0.1:{port}"))
        .replace("{data}", &data.display().to_string());
    let mut words = line.split_whitespace();
    let program = words.next().ok_or("--cmd is empty")?;
    let child = Command::new(program)
        .args(words)
        .current_dir(&data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("starting {program}: {e}"))?;
    let mut child = child;
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err() {
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err("server did not start".to_string());
        }
        sleep(Duration::from_millis(100));
    }
    Ok((format!("http://127.0.0.1:{port}"), child, data))
}

fn pytest(module: &str, endpoint: &str) -> Result<String, String> {
    let uv = std::env::var("UV_BIN").unwrap_or_else(|_| "uv".to_string());
    let out = Command::new(uv)
        .args([
            "run",
            "--frozen",
            "pytest",
            &format!("tests/test_{module}.py"),
            "-p",
            "no:cacheprovider",
        ])
        .current_dir(DIR)
        .env("CONFORMANCE_ENDPOINT", endpoint)
        .env("CONFORMANCE_TARGET", "doze")
        .output()
        .map_err(|e| format!("uv: {e} (set UV_BIN)"))?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// The number written just before `phrase` on some line, or 0.
fn count_before(text: &str, phrase: &str) -> usize {
    text.lines()
        .find_map(|l| {
            let head = l.split(phrase).next().filter(|_| l.contains(phrase))?;
            head.split_whitespace().last()?.parse().ok()
        })
        .unwrap_or(0)
}

fn score(text: &str) -> Score {
    Score {
        exact: count_before(text, "matched what real AWS answered"),
        differ: count_before(text, "answered differently"),
        unverified: count_before(text, "UNVERIFIED"),
        wording_only: wording_only(text),
    }
}

/// Labels whose every differing path is `$.Error.Message`. The harness prints, per failing test, a header
/// `ERROR at teardown of <test>`, then `[label]` at depth 13 and each differing path at depth 15.
fn wording_only(text: &str) -> usize {
    let mut paths: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let (mut test, mut label) = (String::new(), String::new());
    for line in text.lines() {
        if let Some(rest) = line
            .trim_start_matches('_')
            .strip_prefix(" ERROR at teardown of ")
        {
            test = rest.trim_end_matches(['_', ' ']).to_string();
        } else if let Some(l) = line.strip_prefix(&format!("E{}[", " ".repeat(13))) {
            label = l.trim_end_matches(']').to_string();
        } else if let Some(p) = line
            .strip_prefix(&format!("E{}", " ".repeat(15)))
            .filter(|p| p.starts_with('$'))
        {
            paths
                .entry((test.clone(), label.clone()))
                .or_default()
                .insert(p.trim().to_string());
        }
    }
    paths
        .values()
        .filter(|ps| ps.iter().all(|p| p == "$.Error.Message"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_come_from_the_line_that_names_them() {
        let text = "  423 matched what real AWS answered\n  7 answered differently\n  2 UNVERIFIED (no recording)\n";
        let s = score(text);
        assert_eq!((s.exact, s.differ, s.unverified), (423, 7, 2));
        assert_eq!(count_before("nothing here", "UNVERIFIED"), 0);
    }

    #[test]
    fn a_label_that_differs_only_in_wording_is_counted_apart() {
        let text = format!(
            "___ ERROR at teardown of test_a ___\nE{}[wording]\nE{}$.Error.Message\nE{}[shape]\nE{}$.Error.Message\nE{}$.Error.Code\n",
            " ".repeat(13),
            " ".repeat(15),
            " ".repeat(13),
            " ".repeat(15),
            " ".repeat(15)
        );
        assert_eq!(wording_only(&text), 1);
    }

    #[test]
    fn arguments_are_checked() {
        assert!(parse_args(vec!["--nope".into()]).is_err());
        assert!(parse_args(vec!["--cmd".into()]).is_err());
        let (t, m) =
            parse_args(vec!["--endpoint".into(), "http://x".into(), "sqs".into()]).unwrap();
        assert!(matches!(t, Target::Endpoint(_)));
        assert_eq!(m, ["sqs"]);
    }

    #[test]
    fn percentages_do_not_divide_by_zero() {
        assert_eq!(percent(&Score::default()), "0% exact, 0% ignoring wording");
    }
}
