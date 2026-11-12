//! What the Live pages ask of the source: the tape, a request, the clients, the first request, the docks, Home's strip. A live
//! console reads the recorder (and, for a drawer, the topic's config as the broker says it now); the fixtures answer with the
//! boards' world. A console started without a recorder (a test that wants none) answers an empty tape, honestly.

use std::sync::Arc;

use super::{Live, Source, now_ms};
use crate::gather;
use crate::model::{
    ClientsView, FirstView, GroupWireView, LiveState, LiveView, SpokeView, WireDetail, WireQuery,
    WireRow, WireWindow,
};
use crate::pages::live::RequestLimits;
use crate::recorder::Recorder;

impl Live {
    /// The same source reading `recorder`.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Arc<Recorder>) -> Self {
        self.recorder = Some(recorder);
        self
    }

    fn tape(&self, q: &WireQuery) -> LiveView {
        let now = now_ms();
        let mut v = self
            .recorder
            .as_ref()
            .map_or_else(|| empty(q, now), |r| r.tape(q, now));
        v.listen = self.facts.bootstrap();
        v.started_ms = self.backend.instance().map(|i| i.started_ms);
        v
    }
}

fn empty(q: &WireQuery, now: i64) -> LiveView {
    Recorder::new().tape(q, now)
}

impl Source {
    /// The recorder, if this source has one: the console's state hands the shared tick to it.
    pub(crate) fn attach(&self, ticker: &Arc<crate::tick::Ticker>) {
        if let Self::Live(l) = self
            && let Some(r) = &l.recorder
        {
            r.attach(Arc::clone(ticker));
        }
    }

    /// The Live page for the filters in `q`. `hint` names a state for the fixtures only (`paused`, `quiet`, `load`).
    pub async fn live(&self, q: &WireQuery, hint: Option<&str>) -> LiveView {
        match self {
            Self::Live(l) => {
                let _ = hint;
                l.tape(q)
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_live::live(q, hint),
        }
    }

    /// One request in full, with what the broker says of its topic now (for a refusal's rules). `None`: not on the tape any more.
    pub async fn request(&self, id: u64) -> Option<(WireDetail, RequestLimits)> {
        match self {
            Self::Live(l) => {
                let d = l.recorder.as_ref()?.get(id)?;
                let limits = match d.topics.first() {
                    Some((t, _)) if matches!(d.row.code, 10 | 87) => {
                        gather::request_limits(&l.backend, t).await
                    }
                    _ => RequestLimits::default(),
                };
                Some((d, limits))
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_live::request_by_id(id),
        }
    }

    /// The clients connected now.
    pub async fn clients(&self) -> ClientsView {
        match self {
            Self::Live(l) => l.recorder.as_ref().map_or(
                ClientsView {
                    rows: vec![],
                    connections: 0,
                    now_ms: now_ms(),
                },
                |r| r.clients(now_ms()),
            ),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_live::clients(),
        }
    }

    /// The newest client the broker first heard from, with the address and profile the page says.
    pub async fn first(&self) -> Option<FirstView> {
        match self {
            Self::Live(l) => {
                let mut v = l.recorder.as_ref()?.first()?;
                v.listen = l.facts.bootstrap();
                v.profile = format!("Kafka {}", l.facts.kafka_major);
                Some(v)
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => Some(crate::fixtures_live::first()),
        }
    }

    /// The last requests that touched a topic, and what the tape covers.
    pub async fn topic_wire(&self, topic: &str) -> (Vec<WireRow>, WireWindow, i64) {
        match self {
            Self::Live(l) => {
                let q = WireQuery {
                    topic: vec![topic.to_owned()],
                    hide_idle: false,
                    limit: 12,
                    ..WireQuery::default()
                };
                let v = l.tape(&q);
                (v.rows, v.window, v.now_ms)
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => {
                let (r, w) = crate::fixtures_live::topic_wire(topic);
                (r, w, crate::fixtures_live::now())
            }
        }
    }

    /// Every client request that touched a group, and what the tape covers.
    pub async fn group_wire(&self, q: &WireQuery) -> GroupWireView {
        match self {
            Self::Live(l) => {
                let ask = WireQuery {
                    hide_idle: false,
                    limit: 50,
                    ..q.clone()
                };
                let all = l.tape(&WireQuery {
                    kind: String::new(),
                    refused_only: false,
                    text: String::new(),
                    ..ask.clone()
                });
                let v = l.tape(&ask);
                GroupWireView {
                    group: q.group.clone(),
                    query: q.clone(),
                    rows: v.rows,
                    window: v.window,
                    now_ms: v.now_ms,
                    all: all.matched,
                    refused: all.rows.iter().filter(|r| r.class.is_refusal()).count(),
                }
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_live::group_wire(q),
        }
    }

    /// Home's strip: how many clients the tape knows and when the last request landed; `None` with no recorder.
    pub async fn spoke(&self) -> Option<SpokeView> {
        match self {
            Self::Live(l) => l.recorder.as_ref().map(|r| r.spoke()),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => Some(crate::fixtures_live::spoke()),
        }
    }
}

/// Whether a tape holds anything yet (the state words of Home).
#[must_use]
pub fn is_quiet(v: &LiveView) -> bool {
    v.state == LiveState::Quiet
}
