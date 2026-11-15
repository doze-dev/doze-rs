//! Opening the app at login, in the menu bar only (Go `internal/desk/autostart`): a LaunchAgent on macOS, an XDG
//! autostart entry on Linux. It writes and removes one file in the person's own folders, needs no privilege, and asks
//! launchd nothing: the agent runs at the next login. Windows has none yet.
//!
//! Every function takes the folder the file lives under, so a test (or a fixture run of the app) points it at a
//! temporary folder and never touches the person's real login items.

use std::io;
use std::path::{Path, PathBuf};

/// Names the login item on every platform.
pub const LABEL: &str = "dev.doze.app";

/// Where the login item lives under `user_home` (`config` is `$XDG_CONFIG_HOME`, used on Linux when set). `None` on a
/// platform with no login items doze knows how to write.
pub fn path_in(user_home: &Path, config: Option<&Path>) -> Option<PathBuf> {
    imp::path_in(user_home, config)
}

/// The login item is in place.
pub fn enabled(path: &Path) -> bool {
    path.is_file()
}

/// Puts the login item at `path`, starting `exe` with `args` (the menu bar only), or removes it.
pub fn set(path: &Path, on: bool, exe: &Path, args: &[&str]) -> io::Result<()> {
    if !on {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, imp::entry(&exe.to_string_lossy(), args))?;
    std::fs::rename(&tmp, path)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // WHY: the macOS form; its test runs everywhere
/// A LaunchAgent that runs the app once at login. It is not kept alive: quitting the app from its menu means quit.
fn plist(exe: &str, args: &[&str]) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut argv = format!("    <string>{}</string>\n", esc(exe));
    for a in args {
        argv.push_str(&format!("    <string>{}</string>\n", esc(a)));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{argv}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
"#
    )
}

#[cfg_attr(target_os = "macos", allow(dead_code))] // WHY: the Linux form; its test runs everywhere
/// An XDG autostart entry.
fn desktop(exe: &str, args: &[&str]) -> String {
    let quote = |s: &str| {
        let s = s.replace('%', "%%");
        if s.contains([' ', '\t', '"', '`', '$', '\\']) {
            format!(
                "\"{}\"",
                s.replace('\\', r"\\")
                    .replace('"', "\\\"")
                    .replace('`', "\\`")
                    .replace('$', "\\$")
            )
        } else {
            s
        }
    };
    let mut exec = quote(exe);
    for a in args {
        exec.push(' ');
        exec.push_str(&quote(a));
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=doze\nComment=doze in the menu bar\nExec={exec}\nIcon=doze\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    pub(super) fn path_in(home: &Path, _config: Option<&Path>) -> Option<PathBuf> {
        Some(
            home.join("Library/LaunchAgents")
                .join(format!("{}.plist", super::LABEL)),
        )
    }

    pub(super) fn entry(exe: &str, args: &[&str]) -> String {
        super::plist(exe, args)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::path::{Path, PathBuf};

    pub(super) fn path_in(home: &Path, config: Option<&Path>) -> Option<PathBuf> {
        if cfg!(windows) {
            return None;
        }
        let dir = config.map_or_else(|| home.join(".config"), Path::to_path_buf);
        Some(
            dir.join("autostart")
                .join(format!("{}.desktop", super::LABEL)),
        )
    }

    pub(super) fn entry(exe: &str, args: &[&str]) -> String {
        super::desktop(exe, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_item_is_written_and_removed_in_the_folder_given() {
        let home = tempfile::tempdir().unwrap();
        let Some(p) = path_in(home.path(), None) else {
            return;
        };
        assert!(p.starts_with(home.path()));
        assert!(!enabled(&p));
        set(
            &p,
            true,
            Path::new("/Applications/Doze.app/Contents/MacOS/doze-app"),
            &["--menu-bar"],
        )
        .unwrap();
        assert!(enabled(&p));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("doze-app") && text.contains("--menu-bar"));
        set(&p, false, Path::new("x"), &[]).unwrap();
        assert!(!enabled(&p));
        set(&p, false, Path::new("x"), &[]).unwrap();
    }

    #[test]
    fn both_formats_quote_what_they_must() {
        let p = plist("/a & b/<doze>", &["--menu-bar"]);
        assert!(p.contains("<string>/a &amp; b/&lt;doze&gt;</string>"));
        let d = desktop("/opt/my apps/doze $x", &["--menu-bar"]);
        assert!(d.contains("Exec=\"/opt/my apps/doze \\$x\" --menu-bar"));
    }
}
