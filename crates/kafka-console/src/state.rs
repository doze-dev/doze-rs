//! What every handler shares.

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::base::Base;
use crate::limits::Limits;
use crate::source::Source;

/// The state of one console.
#[derive(Debug)]
pub struct State {
    pub source: Source,
    /// The console's root (`--console-prefix`): the console, its API and its files are under it.
    pub web: Base,
    /// The console's built files.
    pub bundle: crate::web::Bundle,
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
    /// [`State::console`] with the default limits.
    #[must_use]
    pub fn new(
        source: Source,
        web: Base,
        allow_hosts: Vec<String>,
        stop: CancellationToken,
    ) -> Shared {
        Self::console(source, &web, allow_hosts, stop, &Limits::default())
    }

    /// The state of the console served at `web` (`--console-prefix`).
    #[must_use]
    pub fn console(
        source: Source,
        web: &Base,
        allow_hosts: Vec<String>,
        stop: CancellationToken,
        limits: &Limits,
    ) -> Shared {
        Arc::new(Self {
            bundle: crate::web::Bundle::build(web),
            source,
            web: web.clone(),
            stop,
            allow_hosts,
            streams: Arc::new(Semaphore::new(limits.max_streams)),
        })
    }
}
