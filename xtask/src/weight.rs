//! `cargo xtask weight [package ...]`: builds a package in release mode and measures what it weighs:
//! binary size, packages linked, and resident memory at idle. Each package has a line in
//! `xtask/weight-limits.txt`; a number over its limit fails. Weight is a feature (docs/port/weight.md):
//! a limit only moves with a written reason.
//!
//! The limits file has one package per line: `package max_bytes max_packages max_rss_kb args...`, where
//! the args start the server and may use `{port}` and `{data}`. Lines starting with `#` are comments. A line with no
//! args is a program that serves nothing by itself (the `doze` CLI): its size and packages are weighed, its idle memory
//! is not (write 0).

use std::collections::BTreeSet;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const LIMITS: &str = "xtask/weight-limits.txt";

/// What one package may weigh, and how to start it.
#[derive(Debug, PartialEq, Eq)]
struct Limit {
    package: String,
    max_bytes: u64,
    max_packages: usize,
    max_rss_kb: u64,
    args: Vec<String>,
}

/// What one package weighs.
#[derive(Debug, PartialEq, Eq)]
struct Measure {
    bytes: u64,
    packages: usize,
    rss_kb: u64,
}

pub(crate) fn run(wanted: &[String]) -> ExitCode {
    let text = match fs::read_to_string(LIMITS) {
        Ok(t) => t,
        Err(e) => return fail(&format!("reading {LIMITS}: {e}")),
    };
    let limits = match parse_limits(&text) {
        Ok(l) => l,
        Err(e) => return fail(&format!("{LIMITS}: {e}")),
    };
    let chosen: Vec<&Limit> = limits
        .iter()
        .filter(|l| wanted.is_empty() || wanted.contains(&l.package))
        .collect();
    if chosen.is_empty() {
        return fail("no package matches (see the limits file)");
    }
    println!(
        "{:<16} {:>12} {:>9} {:>10}   verdict",
        "package", "bytes", "packages", "idle KB"
    );
    let mut failed = false;
    for limit in chosen {
        match measure(limit) {
            Ok(m) => {
                let breaches = breaches(&m, limit);
                println!(
                    "{:<16} {:>12} {:>9} {:>10}   {}",
                    limit.package,
                    m.bytes,
                    m.packages,
                    m.rss_kb,
                    if breaches.is_empty() {
                        "ok".to_string()
                    } else {
                        breaches.join("; ")
                    }
                );
                failed |= !breaches.is_empty();
            }
            Err(e) => {
                println!("{:<16} {e}", limit.package);
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("xtask weight: {message}");
    ExitCode::FAILURE
}

fn parse_limits(text: &str) -> Result<Vec<Limit>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(package), Some(bytes), Some(packages), Some(rss)) =
            (words.next(), words.next(), words.next(), words.next())
        else {
            return Err(format!(
                "line {}: expected package, bytes, packages, rss_kb",
                n + 1
            ));
        };
        let number = |s: &str| {
            s.parse::<u64>()
                .map_err(|_| format!("line {}: `{s}` is not a number", n + 1))
        };
        out.push(Limit {
            package: package.to_string(),
            max_bytes: number(bytes)?,
            max_packages: usize::try_from(number(packages)?).map_err(|e| e.to_string())?,
            max_rss_kb: number(rss)?,
            args: words.map(ToString::to_string).collect(),
        });
    }
    Ok(out)
}

fn breaches(m: &Measure, limit: &Limit) -> Vec<String> {
    let mut out = Vec::new();
    if m.bytes > limit.max_bytes {
        out.push(format!("binary over {} B", limit.max_bytes));
    }
    if m.packages > limit.max_packages {
        out.push(format!("more than {} packages", limit.max_packages));
    }
    if m.rss_kb > limit.max_rss_kb {
        out.push(format!("idle memory over {} KB", limit.max_rss_kb));
    }
    out
}

/// The binary a package builds, when it is not named after the package.
fn binary_of(package: &str) -> &str {
    match package {
        "doze-cli" => "doze",
        other => other,
    }
}

/// The build profile a package is weighed in: doze-aws has its own, smaller one (see the workspace `Cargo.toml`).
fn profile_of(package: &str) -> &'static str {
    if package == "doze-aws" {
        "release-aws"
    } else {
        "release"
    }
}

fn measure(limit: &Limit) -> Result<Measure, String> {
    let profile = profile_of(&limit.package);
    let status = Command::new("cargo")
        .args(["build", "--profile", profile, "-p", &limit.package])
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("release build failed".to_string());
    }
    let binary = Path::new("target").join(profile).join(format!(
        "{}{}",
        binary_of(&limit.package),
        std::env::consts::EXE_SUFFIX
    ));
    let bytes = fs::metadata(&binary)
        .map_err(|e| format!("{}: {e}", binary.display()))?
        .len();
    Ok(Measure {
        bytes,
        packages: count_packages(&limit.package)?,
        rss_kb: if limit.args.is_empty() {
            0
        } else {
            idle_rss_kb(&binary, &limit.args)?
        },
    })
}

/// Unique packages in the normal dependency tree, ours included.
fn count_packages(package: &str) -> Result<usize, String> {
    let out = Command::new("cargo")
        .args([
            "tree", "-p", package, "--prefix", "none", "--edges", "normal",
        ])
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(unique_lines(&text))
}

fn unique_lines(text: &str) -> usize {
    let lines = text.lines().filter(|l| !l.trim().is_empty());
    lines.collect::<BTreeSet<_>>().len()
}

/// Starts the server on a free port with an empty data directory, waits until it accepts, lets it settle,
/// and reads its resident set from `ps` (the same on Linux and macOS).
fn idle_rss_kb(binary: &Path, args: &[String]) -> Result<u64, String> {
    let port = free_port()?;
    let data = std::env::temp_dir().join(format!("xtask-weight-{}-{port}", std::process::id()));
    fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let args: Vec<String> = args
        .iter()
        .map(|a| {
            a.replace("{port}", &port.to_string())
                .replace("{data}", &data.display().to_string())
        })
        .collect();
    let mut child = Command::new(binary)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("starting {}: {e}", binary.display()))?;
    let result = wait_until_up(port).and_then(|()| {
        sleep(Duration::from_secs(2));
        read_rss_kb(child.id())
    });
    let _ = child.kill();
    let _ = child.wait();
    let _ = fs::remove_dir_all(&data);
    result
}

fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    Ok(listener.local_addr().map_err(|e| e.to_string())?.port())
}

fn wait_until_up(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(100));
    }
    Err(format!(
        "the server did not listen on port {port} within 20 s"
    ))
}

fn read_rss_kb(pid: u32) -> Result<u64, String> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .map_err(|e| format!("ps: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .map_err(|_| "ps gave no resident size".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "# comment\n\ndoze-kafka 5150000 115 8192 --listen 127.0.0.1:{port} --data-dir {data}\n";

    #[test]
    fn a_limit_line_is_read() {
        let limits = parse_limits(SAMPLE).unwrap();
        assert_eq!(limits.len(), 1);
        let l = &limits[0];
        assert_eq!(
            (
                l.package.as_str(),
                l.max_bytes,
                l.max_packages,
                l.max_rss_kb
            ),
            ("doze-kafka", 5_150_000, 115, 8192)
        );
        assert_eq!(
            l.args,
            ["--listen", "127.0.0.1:{port}", "--data-dir", "{data}"]
        );
    }

    #[test]
    fn bad_lines_are_named() {
        assert!(
            parse_limits("doze-kafka 1 2")
                .unwrap_err()
                .contains("line 1")
        );
        assert!(
            parse_limits("\ndoze-kafka x 2 3")
                .unwrap_err()
                .contains("line 2")
        );
    }

    #[test]
    fn only_a_number_over_its_limit_is_a_breach() {
        let limit = parse_limits(SAMPLE).unwrap().remove(0);
        let at = Measure {
            bytes: 5_150_000,
            packages: 115,
            rss_kb: 8192,
        };
        assert!(breaches(&at, &limit).is_empty());
        let over = Measure {
            bytes: 5_150_001,
            packages: 116,
            rss_kb: 8193,
        };
        assert_eq!(breaches(&over, &limit).len(), 3);
    }

    #[test]
    fn a_package_listed_twice_in_the_tree_counts_once() {
        assert_eq!(unique_lines("a v1\nb v1\na v1\n\n"), 2);
    }
}
