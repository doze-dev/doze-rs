//! Where the pages get their view-models: a live broker, or (in tests and the `boards` example) the "larder" fixtures.
//!
//! Handlers ask the source for a model and render it; they never know which kind it is.

use crate::backend::Backend;
use crate::facts::{self, Facts};
use crate::format::date_time;
use crate::gather;
use crate::model::{Access, ConnectView, HomeView, ProbeView};
use crate::probe::Prober;
use crate::snippets;
use crate::ui::shell::Chrome;

mod groups;
mod live;
mod settings;
mod topics;

pub use live::is_quiet;

/// A live broker: the in-process handle, how it was started, and the advertised-listener check.
#[derive(Debug)]
pub struct Live {
    pub backend: Backend,
    pub facts: Facts,
    pub prober: Prober,
    /// What the broker was asked, if the console was started with a recorder (slice 9e).
    pub recorder: Option<std::sync::Arc<crate::recorder::Recorder>>,
    /// The flags and the rest of how the binary started the broker (empty where not known).
    pub start: crate::facts_start::StartFacts,
}

impl Live {
    #[must_use]
    pub fn new(backend: Backend, facts: Facts) -> Self {
        let prober = Prober::new(facts.clone(), backend.clone());
        Self {
            backend,
            facts,
            prober,
            recorder: None,
            start: crate::facts_start::StartFacts::default(),
        }
    }

    /// The same, told how the binary started the broker: the Settings and Access pages say it.
    #[must_use]
    pub fn with_start(mut self, start: crate::facts_start::StartFacts) -> Self {
        self.start = start;
        self
    }

    async fn access(&self) -> Access {
        let (acls, allow) = self
            .backend
            .instance()
            .map_or((0, true), |i| (i.acls, i.acl_default_allow));
        facts::access(&self.facts, acls, allow)
    }

    /// Home. `dial`: whether the advertised-listener result may be made now (a page load: a result under 30 s old is
    /// reused) or is only what the last check said (the live fragment, which never dials).
    async fn home(&self, dial: bool) -> HomeView {
        let now = now_ms();
        let (topics, partitions) = gather::counts(&self.backend).await;
        let looked = gather::nearest(&self.backend, now).await;
        let access = self.access().await;
        let instance = self
            .backend
            .instance()
            .map(|i| gather::instance_view(&i, &self.facts, access, now));
        let probe = if dial {
            self.prober.cached().await
        } else {
            self.prober.peek()
        };
        let instance = instance.unwrap_or_else(|| unknown_instance(&self.facts, &probe));
        HomeView {
            state: gather::choose(topics, looked.attention),
            listen: self.facts.bootstrap(),
            loopback: self.facts.loopback(),
            topics,
            partitions,
            probe,
            instance,
            unchecked: looked.unchecked,
            spoke: self.recorder.as_ref().map(|r| r.spoke()),
        }
    }
}

/// The milliseconds since the epoch now.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The instance card for a caller the broker will not describe the cluster to: only what the flags say.
fn unknown_instance(facts: &Facts, _probe: &ProbeView) -> crate::model::InstanceView {
    crate::model::InstanceView {
        started: date_time(now_ms()),
        up_secs: 0,
        last_stop: crate::model::StopView::None,
        open_partitions: 0,
        max_open_partitions: 0,
        open_files: None,
        resident_bytes: None,
        threads: None,
        profile: format!("Kafka {}", facts.kafka_major),
        access: facts::access(facts, 0, true),
        data_dir: String::new(),
    }
}

/// The data of the pages.
#[derive(Debug)]
pub enum Source {
    Live(Box<Live>),
    #[cfg(any(test, feature = "examples"))]
    Fixture,
}

impl Source {
    /// The fragments a Home page asks the stream to watch: none over the fixtures, which do not change and which a
    /// refetch would turn back into the steady state.
    #[must_use]
    pub fn watch(&self) -> &'static [&'static str] {
        match self {
            Self::Live(_) => crate::stream::HOME_IDS,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => &[],
        }
    }

    /// The top bar.
    pub async fn chrome(&self) -> Chrome {
        match self {
            Self::Live(l) => facts::chrome(&l.facts, &l.access().await),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures::chrome(),
        }
    }

    /// Home's live fragment: the same view, with the advertised-listener part as the last check left it (no dial).
    pub async fn home_live(&self) -> HomeView {
        match self {
            Self::Live(l) => l.home(false).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => self.home(None).await,
        }
    }

    /// Home. `hint` names a state (`quiet`, `attention`) for the fixtures only; a live broker decides by its data.
    pub async fn home(&self, hint: Option<&str>) -> HomeView {
        match self {
            Self::Live(l) => {
                let _ = hint;
                l.home(true).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures::home(match hint {
                Some("quiet") => crate::model::HomeState::Quiet,
                Some("attention") => {
                    crate::model::HomeState::Attention(crate::fixtures::attention())
                }
                _ => crate::model::HomeState::Steady,
            }),
        }
    }

    /// Connect.
    pub async fn connect(&self) -> ConnectView {
        match self {
            Self::Live(l) => {
                let access = l.access().await;
                ConnectView {
                    listen: l.facts.bootstrap(),
                    loopback: l.facts.loopback(),
                    probe: l.prober.cached().await,
                    access,
                    snippets: snippets::for_address(&l.facts.bootstrap()),
                }
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures::connect(),
        }
    }

    /// The advertised-listener check, from the last result or a new one; `fresh` forces a new one.
    pub async fn probe(&self, fresh: bool) -> ProbeView {
        match self {
            Self::Live(l) if fresh => l.prober.run().await,
            Self::Live(l) => l.prober.cached().await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures::probe_ok(),
        }
    }
}
