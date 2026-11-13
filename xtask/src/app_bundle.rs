//! `cargo xtask app-bundle`: Doze.app around the desktop app's binary (docs/port/app/PLAN.md step 3.5; the Go app's
//! `build/darwin/bundle.sh`), then signed.
//!
//! ```text
//! cargo xtask app-bundle [--profile release-app] [--out DIR] [--version V] [--id ID] [--name NAME] [--no-scheme]
//!                        [--no-build] [--features F]
//! ```
//!
//! - The binary is `doze-app`, built with `--profile` (default `release-app`) unless `--no-build`.
//! - `Info.plist` comes from `apps/doze-app/bundle/` (LSUIElement: a menu-bar app until a window opens; the `doze://`
//!   scheme unless `--no-scheme`); the icon is the doze mark (`apps/doze-app/assets/brand/doze.svg`) rendered by `sips`
//!   at every size `iconutil` wants.
//! - Signing: with `DOZE_SIGN_ID` (a Developer ID in the keychain) it signs with the hardened runtime and the
//!   entitlements, and with `NOTARY_KEY_PATH`, `NOTARY_KEY_ID` and `NOTARY_ISSUER_ID` it notarises and staples; with
//!   neither it signs ad hoc, which is enough to run here. CI signs only when its secrets exist.
//! - `--id` and `--name` make a bundle that is not the real app's identity (a test bundle, so it never takes the
//!   owner's `dev.doze.app` or its `doze://` links); `--no-scheme` leaves the link scheme out for the same reason.
//!
//! The bundle is written to `--out` (default `<target>/bundle/Doze.app`); nothing is installed anywhere.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// What to make.
struct Opts {
    profile: String,
    out: Option<PathBuf>,
    version: String,
    id: String,
    name: String,
    scheme: bool,
    build: bool,
    features: Option<String>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        profile: "release-app".into(),
        out: None,
        version: "0.0.0".into(),
        id: "dev.doze.app".into(),
        name: "Doze".into(),
        scheme: true,
        build: true,
        features: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--profile" => o.profile = value()?,
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--version" => o.version = value()?,
            "--id" => o.id = value()?,
            "--name" => o.name = value()?,
            "--no-scheme" => o.scheme = false,
            "--no-build" => o.build = false,
            "--features" => o.features = Some(value()?),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    Ok(o)
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    match parse(args).and_then(|o| make(&o)) {
        Ok(app) => {
            println!("app-bundle: {}", app.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("app-bundle: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `cmd`, failing with what it printed when it fails.
fn sh(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{cmd:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from)
}

fn make(o: &Opts) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("Doze.app is a macOS bundle; run this on a Mac".into());
    }
    if o.build {
        sh(
            Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args(["build", "-p", "doze-app", "--profile", &o.profile])
                .args(o.features.iter().flat_map(|f| ["--features", f.as_str()])),
        )?;
    }
    let bin = target_dir().join(&o.profile).join("doze-app");
    let app = o
        .out
        .clone()
        .unwrap_or_else(|| target_dir().join("bundle"))
        .join(format!("{}.app", o.name));
    let _ = std::fs::remove_dir_all(&app);
    let (macos, res) = (app.join("Contents/MacOS"), app.join("Contents/Resources"));
    for d in [&macos, &res] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::copy(&bin, macos.join("doze-app")).map_err(|e| format!("{}: {e}", bin.display()))?;
    let plist = info_plist(o, Path::new("."))?;
    write(&app.join("Contents/Info.plist"), &plist)?;
    write(&app.join("Contents/PkgInfo"), "APPL????")?;
    icon(&res.join("doze.icns"))?;
    sh(Command::new("plutil")
        .arg("-lint")
        .arg(app.join("Contents/Info.plist")))?;
    sign(&app)?;
    Ok(app)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The Info.plist with the names filled in.
fn info_plist(o: &Opts, root: &Path) -> Result<String, String> {
    let dir = root.join("apps/doze-app/bundle");
    let scheme = if o.scheme {
        read(&dir.join("scheme.plist"))?
    } else {
        String::new()
    };
    Ok(read(&dir.join("Info.plist"))?
        .replace("__SCHEME__", &scheme)
        .replace("__NAME__", &o.name)
        .replace("__ID__", &o.id)
        .replace("__VERSION__", &o.version))
}

/// The doze mark as an `.icns`: `sips` renders the SVG at each size, `iconutil` packs them.
fn icon(out: &Path) -> Result<(), String> {
    let set = target_dir().join("bundle/doze.iconset");
    let _ = std::fs::remove_dir_all(&set);
    std::fs::create_dir_all(&set).map_err(|e| format!("{}: {e}", set.display()))?;
    for size in [16u32, 32, 128, 256, 512] {
        for (scale, px) in [("", size), ("@2x", size * 2)] {
            let file = set.join(format!("icon_{size}x{size}{scale}.png"));
            sh(Command::new("sips")
                .args(["-s", "format", "png", "-Z", &px.to_string()])
                .arg("apps/doze-app/assets/brand/doze.svg")
                .arg("--out")
                .arg(&file))?;
        }
    }
    sh(Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(out)
        .arg(&set))
}

/// A Developer ID with the hardened runtime when `DOZE_SIGN_ID` is set (and notarised when the notary's key is too);
/// ad hoc otherwise. Then the signature is checked.
fn sign(app: &Path) -> Result<(), String> {
    let id = std::env::var("DOZE_SIGN_ID").unwrap_or_default();
    if id.is_empty() {
        sh(Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(app))?;
        println!("app-bundle: ad-hoc signed (set DOZE_SIGN_ID for a Developer ID)");
    } else {
        sh(Command::new("codesign")
            .args([
                "--force",
                "--timestamp",
                "--options",
                "runtime",
                "--entitlements",
            ])
            .arg("apps/doze-app/bundle/entitlements.plist")
            .args(["--sign", &id])
            .arg(app))?;
        println!("app-bundle: signed with {id}");
        notarise(app)?;
    }
    sh(Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app))
}

/// Notarises and staples, when `NOTARY_KEY_PATH`, `NOTARY_KEY_ID` and `NOTARY_ISSUER_ID` are all set.
fn notarise(app: &Path) -> Result<(), String> {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let (key, key_id, issuer) = (
        var("NOTARY_KEY_PATH"),
        var("NOTARY_KEY_ID"),
        var("NOTARY_ISSUER_ID"),
    );
    if key.is_empty() || key_id.is_empty() || issuer.is_empty() {
        println!("app-bundle: not notarised (no NOTARY_KEY_PATH, NOTARY_KEY_ID, NOTARY_ISSUER_ID)");
        return Ok(());
    }
    let zip = app.with_extension("app.zip");
    sh(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(&zip))?;
    sh(Command::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(&zip)
        .args([
            "--wait", "--key", &key, "--key-id", &key_id, "--issuer", &issuer,
        ]))?;
    sh(Command::new("xcrun").args(["stapler", "staple"]).arg(app))?;
    let _ = std::fs::remove_file(&zip);
    sh(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(&zip))?;
    println!("app-bundle: notarised and stapled; {}", zip.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{info_plist, parse};

    #[test]
    fn a_test_bundle_takes_its_own_name_and_no_scheme() {
        let args: Vec<String> = [
            "--id",
            "dev.doze.app.test",
            "--name",
            "Doze Test",
            "--no-scheme",
            "--version",
            "1.2.3",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let o = parse(&args).unwrap_or_else(|e| unreachable!("{e}"));
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
        let p = info_plist(&o, root).unwrap_or_default();
        assert!(p.contains("<string>dev.doze.app.test</string>"));
        assert!(p.contains("<string>Doze Test</string>"));
        assert!(p.contains("<string>1.2.3</string>"));
        assert!(!p.contains("CFBundleURLSchemes"));
        assert!(!p.contains("__"));
        assert!(p.contains("<key>LSUIElement</key>\n  <true/>"));
    }
}
