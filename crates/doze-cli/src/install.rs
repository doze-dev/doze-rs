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

/// Where doze's releases are published: the Rust doze goes to doze-dev/doze, as Go's did (owner, 2026-11-19;
/// `.github/workflows/release.yml`). The app's update check (doze-desk `settings::update::LATEST_URL`) asks the same
/// repository.
pub const REPO: &str = "doze-dev/doze";

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

#[cfg(test)]
mod tests {
    use super::{RELEASES, REPO};

    #[test]
    fn the_releases_page_is_the_repository_the_release_workflow_publishes_to() {
        assert_eq!(RELEASES, format!("https://github.com/{REPO}/releases"));
        let workflow = include_str!("../../../.github/workflows/release.yml");
        assert!(
            workflow.contains(&format!("-R {REPO} ")),
            "release.yml publishes to {REPO}"
        );
    }
}
