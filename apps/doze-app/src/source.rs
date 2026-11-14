//! Where the screens' data comes from: the doze-desk hub, always. A real run watches the stacks under the doze home;
//! `--fixture NAME` and `--synth N` (feature `fixtures`) put a fixture daemon on a real socket under a throwaway home
//! and route the hub to it, so a fixture screen goes through the same hub, protocol and folds as a live one.
//!
//! The hub's batches reach the app at most once a frame while the window shows. While it is hidden they still come,
//! once a second ([`HIDDEN`]), for the menu-bar item alone: the window is not told (it is gone, or not visible), so a
//! hidden window costs no redraws.

use std::path::PathBuf;
use std::time::Duration;

use doze_desk::{FRAME, Hub, Options, Policy, StackKey};

/// How often the menu-bar item hears of changes while the window is hidden.
pub(crate) const HIDDEN: Duration = Duration::from_secs(1);

/// What the app reads stacks from.
pub(crate) struct Source {
    pub(crate) hub: Hub,
    /// The stack to show first, when the source knows (a fixture's).
    pub(crate) first: Option<StackKey>,
    /// Where the stopped stacks' shapes and the machine's report come from.
    pub(crate) facts: crate::facts::Mode,
    /// A fixture's daemon and home, kept for as long as the source lives.
    #[cfg(feature = "fixtures")]
    _fixture: Option<crate::fixture::Fixture>,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("first", &self.first)
            .finish_non_exhaustive()
    }
}

/// The hub's pace: a frame while shown; while hidden, once a second, for the tray.
pub(crate) fn policy() -> Policy {
    Policy {
        every: FRAME,
        hidden: Some(HIDDEN),
    }
}

impl Source {
    /// Every stack under the doze home.
    pub(crate) fn live(home: PathBuf) -> Source {
        let mut o = Options::new(home);
        o.policy = policy();
        Source {
            hub: Hub::start(o),
            first: None,
            facts: crate::facts::Mode::Live(crate::facts::find_doze()),
            #[cfg(feature = "fixtures")]
            _fixture: None,
        }
    }

    /// One of the Go fixture hubs (`larder`, `shop`, `crash`, …), or a synthetic stack of `synth` instances.
    #[cfg(feature = "fixtures")]
    pub(crate) fn fixture(name: &str, synth: Option<usize>) -> anyhow::Result<Source> {
        let f = crate::fixture::Fixture::start(name, synth)?;
        let mut o = f.options();
        o.policy = policy();
        let first = f.key.clone();
        Ok(Source {
            hub: Hub::start(o),
            first,
            facts: crate::facts::Mode::Fixture,
            _fixture: Some(f),
        })
    }
}

#[cfg(feature = "fixtures")]
impl Source {
    /// Has the fixture daemon send a frame, as a daemon does when something happens (`bench --logs`).
    pub(crate) fn push(&self, f: &doze_control::Frame) {
        if let Some(fx) = &self._fixture {
            fx.push(f);
        }
    }

    /// Moves the hub's clock on by `ms`, as the app's own (faked) time moves in a test.
    #[cfg(all(test, feature = "capture"))]
    pub(crate) fn skew(&self, ms: u64) {
        if let Some(fx) = &self._fixture {
            fx.skew(ms);
        }
    }

    /// The verbs the fixture daemon was asked, as `op target` (`sleep db`).
    #[cfg(all(test, feature = "capture"))]
    pub(crate) fn verbs(&self) -> Vec<String> {
        self._fixture
            .as_ref()
            .map(crate::fixture::Fixture::requests)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| doze_control::Verb::parse(&r.op).is_some())
            .map(|r| format!("{} {}", r.op, r.db))
            .collect()
    }
}
