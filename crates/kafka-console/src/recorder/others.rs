//! The clients connected, Home's strip and the first-request page.

use std::collections::BTreeMap;

use super::row::Row;
use super::view::{class_of, wire};
use super::{Recorder, names};
use crate::model::{
    ClientRow, ClientsView, FirstView, SettingsHeard, SpokeView, WireClass, WireRow,
};

impl Recorder {
    /// The clients connected now, grouped by `client.id`, most recently active first.
    #[must_use]
    pub fn clients(&self, now_ms: i64) -> ClientsView {
        let inner = self.lock();
        let mut by: BTreeMap<String, Vec<&super::clients::Conn>> = BTreeMap::new();
        for (_, c) in inner.conns.iter() {
            by.entry(c.who.client.to_string()).or_default().push(c);
        }
        let mut rows: Vec<ClientRow> = by
            .into_iter()
            .filter_map(|(client, mut conns)| {
                conns.sort_by_key(|c| std::cmp::Reverse(c.last_ms));
                let newest = *conns.first()?;
                let mut apis: BTreeMap<i16, (i16, u64)> = BTreeMap::new();
                for c in &conns {
                    for (k, (v, n)) in &c.apis {
                        let e = apis.entry(*k).or_insert((*v, 0));
                        e.0 = e.0.max(*v);
                        e.1 += n;
                    }
                }
                let mut apis: Vec<_> = apis
                    .into_iter()
                    .filter_map(|(k, (v, n))| {
                        kafka_wire::messages::ApiKey::try_from(k)
                            .ok()
                            .map(|k| (names::api(k), v, n))
                    })
                    .collect();
                apis.sort_by_key(|(_, _, n)| std::cmp::Reverse(*n));
                Some(ClientRow {
                    tag: newest.who.software.as_deref().and_then(names::tag),
                    software: newest
                        .who
                        .software
                        .as_ref()
                        .map(|s| match &newest.who.version {
                            Some(v) => format!("{s} {v}"),
                            None => s.to_string(),
                        }),
                    peer: newest.who.peer.as_ref().map(ToString::to_string),
                    more: conns.len() - 1,
                    apis: apis.into_iter().map(|(n, v, _)| (n, v)).collect(),
                    principal: newest.who.principal.to_string(),
                    last_ms: newest.last_ms,
                    requests: conns.iter().map(|c| c.requests).sum(),
                    errors: conns.iter().map(|c| c.errors).sum(),
                    client,
                })
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.last_ms));
        ClientsView {
            rows,
            connections: inner.conns.len(),
            now_ms,
        }
    }

    /// Home's strip: how many distinct clients the tape knows, and the last request.
    #[must_use]
    pub fn spoke(&self) -> SpokeView {
        let inner = self.lock();
        let ring = &inner.ring;
        let mut names: Vec<&str> = ring
            .iter()
            .filter(|r| !r.who.client.is_empty())
            .map(|r| &*r.who.client)
            .collect();
        names.sort_unstable();
        names.dedup();
        let last = ring.iter().next_back();
        SpokeView {
            apps: names.len(),
            requests: ring.requests(),
            oldest_ms: ring.iter().next().map(|r| r.first_ms),
            last_ms: last.map(|r| r.last_ms),
            ago_ms: last.map(|r| crate::source::now_ms().saturating_sub(r.last_ms)),
            last_client: last.map(|r| r.who.client.to_string()),
        }
    }

    /// For each of `apis`, the newest client on the tape that used it (a client with no id is nobody) and the version of that
    /// request, whether or not it was answered well: what a client speaks is what it sent. The tape is the last 2,000 rows,
    /// so a client that spoke long ago is not named; an API nobody used is left out.
    #[must_use]
    pub fn heard(&self, apis: &[kafka_wire::messages::ApiKey]) -> Vec<SettingsHeard> {
        let inner = self.lock();
        apis.iter()
            .filter_map(|api| {
                let r = inner
                    .ring
                    .iter()
                    .rev()
                    .find(|r| r.api == *api && !r.who.client.is_empty())?;
                Some(SettingsHeard {
                    client: r.who.client.to_string(),
                    api: names::api(*api),
                    version: r.version,
                })
            })
            .collect()
    }

    /// The clients the tape saw write with each of the producer ids `pids` (a Produce, or its answer, carried the id), as
    /// `client (tag, tag)` with the library tags its ApiVersions gave, newest first and at most three each; a producer the tape
    /// never saw write is not in the answer.
    #[must_use]
    pub fn seen_as(&self, pids: &[i64]) -> BTreeMap<i64, Vec<String>> {
        let inner = self.lock();
        let mut seen: BTreeMap<i64, Vec<(String, Vec<String>)>> = BTreeMap::new();
        for r in inner.ring.iter().rev().filter(|r| !r.who.client.is_empty()) {
            let Some((pid, _)) = r.facts.producer else {
                continue;
            };
            if !pids.contains(&pid) {
                continue;
            }
            let list = seen.entry(pid).or_default();
            let tag = r.who.software.as_deref().and_then(names::tag);
            if let Some((_, tags)) = list.iter_mut().find(|(c, _)| **c == *r.who.client) {
                if let Some(t) = tag.filter(|t| !tags.contains(t)) {
                    tags.push(t);
                }
            } else if list.len() < 3 {
                list.push((r.who.client.to_string(), tag.into_iter().collect()));
            }
        }
        seen.into_iter()
            .map(|(pid, v)| {
                let said = v
                    .into_iter()
                    .map(|(c, tags)| {
                        if tags.is_empty() {
                            c
                        } else {
                            format!("{c} ({})", tags.join(", "))
                        }
                    })
                    .collect();
                (pid, said)
            })
            .collect()
    }

    /// The newest client the tape first heard of, and what it asked, for the first-request page.
    #[must_use]
    pub fn first(&self) -> Option<FirstView> {
        let inner = self.lock();
        let ring = &inner.ring;
        // The client whose first row is the newest: the one that arrived last.
        let mut firsts: BTreeMap<&str, u64> = BTreeMap::new();
        for r in ring.iter().filter(|r| !r.who.client.is_empty()) {
            firsts.entry(&r.who.client).or_insert(r.id);
        }
        let (client, _) = firsts.into_iter().max_by_key(|(_, id)| *id)?;
        let mine: Vec<&Row> = ring.iter().filter(|r| &*r.who.client == client).collect();
        let first = mine.first()?;
        let rows: Vec<WireRow> = mine.iter().take(12).map(|r| wire(r)).collect();
        let topics_listed = mine
            .iter()
            .find(|r| r.api == kafka_wire::messages::ApiKey::Metadata)
            .map(|r| r.facts.items);
        let mut versions: BTreeMap<i16, (&'static str, i16)> = BTreeMap::new();
        for r in &mine {
            let e = versions
                .entry(r.api as i16)
                .or_insert((names::api(r.api), r.version));
            e.1 = e.1.max(r.version);
        }
        Some(FirstView {
            client: client.to_owned(),
            software: mine
                .iter()
                .find_map(|r| r.who.software.as_ref().map(|s| s.to_string())),
            peer: first.who.peer.as_ref().map(ToString::to_string),
            first_ms: first.first_ms,
            all_answered: mine.iter().all(|r| class_of(r) == WireClass::Done),
            refused: mine.iter().filter(|r| class_of(r).is_refusal()).count(),
            rows,
            topics_listed,
            profile: String::new(),
            listen: String::new(),
            versions: versions.into_values().collect(),
        })
    }
}
