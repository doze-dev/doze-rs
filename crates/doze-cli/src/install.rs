//! How this doze was installed, read from where its program is, so "update" can name the exact command
//! (compat.md rule 4: "the exact command for how they installed doze").

use std::path::Path;

/// The ways doze arrives on a machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// Homebrew's formula: `brew upgrade doze`.
    Brew,
    /// Doze.app (the download or the cask): the app updates itself.
    App,
    /// Anything else: a release archive unpacked by hand, a build from source.
    Manual,
}

/// The releases page, for an install doze cannot update itself.
pub const RELEASES: &str = "https://github.com/doze-dev/doze/releases";

impl Install {
    /// Reads the install from the program's path.
    pub fn of(exe: Option<&Path>) -> Install {
        let path = exe
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if path.contains(".app/Contents/") {
            Install::App
        } else if ["/Cellar/", "/homebrew/", "/linuxbrew/"]
            .iter()
            .any(|m| path.contains(m))
        {
            Install::Brew
        } else {
            Install::Manual
        }
    }

    /// How to update, in a few words a person can act on.
    pub fn how(self) -> &'static str {
        match self {
            Install::Brew => "brew upgrade doze",
            Install::App => "in the app: Settings › About",
            Install::Manual => RELEASES,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Install::Brew => "Homebrew",
            Install::App => "the app",
            Install::Manual => "a download",
        }
    }
}
