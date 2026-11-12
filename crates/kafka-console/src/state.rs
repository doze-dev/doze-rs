//! What every handler shares.

use std::sync::Arc;

use maud::{Markup, html};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::assets::Assets;
use crate::base::Base;
use crate::limits::Limits;
use crate::source::Source;
use crate::tick::{FLOOR, Ticker};
use crate::ui::overlay::{Over, over};
use crate::ui::shell::{Crumb, Page, Rail, document_over};

/// The state of one console.
#[derive(Debug)]
pub struct State {
    pub source: Source,
    pub base: Base,
    pub assets: Assets,
    pub ticker: Arc<Ticker>,
    /// Cancelled when the console is told to stop: every open stream ends with it.
    pub stop: CancellationToken,
    /// `--console-allow-host`.
    pub allow_hosts: Vec<String>,
    /// One permit per event stream that may still be opened (the cap is [`Limits::max_streams`]).
    pub streams: Arc<Semaphore>,
}

/// The state, shared.
pub type Shared = Arc<State>;

impl State {
    #[must_use]
    pub fn new(
        source: Source,
        base: Base,
        allow_hosts: Vec<String>,
        stop: CancellationToken,
    ) -> Shared {
        Self::with_limits(source, base, allow_hosts, stop, &Limits::default())
    }

    /// [`State::new`] with the limits given.
    #[must_use]
    pub fn with_limits(
        source: Source,
        base: Base,
        allow_hosts: Vec<String>,
        stop: CancellationToken,
        limits: &Limits,
    ) -> Shared {
        Arc::new(Self {
            assets: Assets::build(&base),
            source,
            base,
            ticker: Ticker::new(FLOOR),
            stop,
            allow_hosts,
            streams: Arc::new(Semaphore::new(limits.max_streams)),
        })
    }

    /// A page in the shell. `crumbs` are label and path (under the base); the last has none.
    pub async fn document(
        &self,
        active: Rail,
        title: &str,
        crumbs: &[(&str, Option<&str>)],
        watch: &[&'static str],
        main: Markup,
    ) -> Markup {
        self.document_over(active, title, crumbs, watch, over(main, html! {}))
            .await
    }

    /// A page with an overlay open on it: the parent's `main` and the overlay element (`ui::overlay`).
    pub async fn document_over(
        &self,
        active: Rail,
        title: &str,
        crumbs: &[(&str, Option<&str>)],
        watch: &[&'static str],
        over: Over,
    ) -> Markup {
        let chrome = self.source.chrome().await;
        let crumbs = crumbs
            .iter()
            .map(|(label, path)| Crumb {
                label: (*label).to_owned(),
                href: path.map(|p| self.base.url(p)),
            })
            .collect();
        let page = Page {
            base: &self.base,
            assets: &self.assets.urls,
            chrome: &chrome,
            active,
            crumbs,
            title,
            watch,
        };
        document_over(&page, over)
    }
}
