//! Packages one built Rust doze module for one target into a registry release directory (docs/port/modules/PLAN.md
//! step 6): the archive, the merged schema-1 index (protocol 2), and meta.yaml. It publishes nothing.

mod index;
mod meta;
mod package;

use doze_module_proto::msg::hello::Manifest;
use std::path::PathBuf;

/// The first doze whose core speaks protocol 2. Set when the Rust core's version is decided; until then a guess the
/// release shows, not one any published doze relies on.
const MIN_DOZE: &str = "0.3.0";

/// What to package.
#[derive(Debug, Clone)]
pub struct Args {
    pub binary: PathBuf,
    pub describe_with: PathBuf,
    pub target: String,
    pub version: String,
    pub out: PathBuf,
    pub namespace: String,
    pub accepted: bool,
}

/// The command line.
pub fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let val = |flag: &str| {
        argv.iter()
            .position(|a| a == flag)
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let need = |flag: &str| val(flag).ok_or_else(|| format!("{flag} is required"));
    let binary = PathBuf::from(need("--binary")?);
    Ok(Args {
        describe_with: val("--describe-with").map_or_else(|| binary.clone(), PathBuf::from),
        binary,
        target: need("--target")?,
        version: need("--version")?,
        out: PathBuf::from(need("--out")?),
        namespace: val("--namespace").unwrap_or_else(|| "doze".into()),
        accepted: argv.iter().any(|a| a == "--accepted"),
    })
}

fn describe(program: &PathBuf) -> Result<Manifest, String> {
    let out = std::process::Command::new(program)
        .arg("--describe")
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if !out.status.success() {
        return Err(format!(
            "{} --describe: {}",
            program.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    doze_module_proto::msg::decode(&out.stdout)
        .map_err(|e| format!("{} --describe: {e}", program.display()))
}

/// Packages one module for one target; returns a one-line summary.
pub fn release(a: &Args) -> Result<String, String> {
    let m = describe(&a.describe_with)?;
    if !m.targets.contains(&a.target) {
        return Err(format!(
            "{} does not declare {} among its targets ({}): not publishing it there",
            m.engine,
            a.target,
            m.targets.join(", ")
        ));
    }
    if !a.accepted {
        return Err(format!(
            "acceptance has not passed for {} on {}: not publishing (--accepted)",
            m.engine, a.target
        ));
    }
    let exe = if a.target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let member = format!("bin/{}-plugin{exe}", m.engine);
    let binary = std::fs::read(&a.binary).map_err(|e| format!("{}: {e}", a.binary.display()))?;
    let (bytes, sha) = package::archive(&member, &binary)?;
    let dir = a.out.join(&m.engine);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let index_path = dir.join("index.yaml");
    let mut idx = match std::fs::read_to_string(&index_path) {
        Ok(text) => index::parse(&text)?,
        Err(_) => index::Index {
            module: m.engine.clone(),
            namespace: a.namespace.clone(),
            ..index::Index::default()
        },
    };
    let archive = format!("{}-plugin-{}-{}.tar.gz", m.engine, a.version, a.target);
    let addition = index::Addition {
        version: a.version.clone(),
        target: a.target.clone(),
        archive: archive.clone(),
        sha256: sha.clone(),
        protocol: i64::from(doze_module_proto::version::CURRENT.major),
        engines: m.versions.clone(),
        min_doze: MIN_DOZE.into(),
    };
    index::add(&mut idx, &addition)?;
    std::fs::write(dir.join(&archive), &bytes).map_err(|e| e.to_string())?;
    std::fs::write(&index_path, index::render(&idx)?).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("meta.yaml"), meta::render(&m)?).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} {} {}: {archive} sha256:{sha}",
        m.engine, a.version, a.target
    ))
}
