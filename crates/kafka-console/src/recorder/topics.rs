//! What the topic pages learn from the tape: who wrote to a topic, a producer's epochs, the groups a transaction staged
//! offsets for, and who wrote an abort marker. Each is as far as the tape reaches (2,000 rows or 1 MiB) and says nothing past it.

use std::collections::BTreeMap;

use kafka_wire::messages::ApiKey;

use super::Recorder;
use crate::model::{AbortBy, ProdEpoch};

impl Recorder {
    /// The clients the tape saw write to each of `topics` (a Produce that named the topic and was not refused), newest first
    /// and at most three each; a topic the tape never saw written is not in the answer.
    #[must_use]
    pub fn writers(&self, topics: &[String]) -> BTreeMap<String, Vec<String>> {
        let inner = self.lock();
        // Per topic, each client and the newest time it wrote (a folded row carries its newest request in `last_ms`).
        let mut last: BTreeMap<&str, BTreeMap<&str, i64>> = BTreeMap::new();
        for r in inner.ring.iter() {
            if r.api != ApiKey::Produce
                || r.who.client.is_empty()
                || r.code != 0
                || r.closed.is_some()
            {
                continue;
            }
            for t in &r.facts.topics {
                if topics.iter().any(|n| **n == *t.name) {
                    let at = last
                        .entry(&t.name)
                        .or_default()
                        .entry(&r.who.client)
                        .or_insert(r.last_ms);
                    *at = (*at).max(r.last_ms);
                }
            }
        }
        last.into_iter()
            .map(|(topic, by)| {
                let mut clients: Vec<(&str, i64)> = by.into_iter().collect();
                clients.sort_by_key(|(c, ms)| (std::cmp::Reverse(*ms), *c));
                let names = clients
                    .into_iter()
                    .take(3)
                    .map(|(c, _)| c.to_owned())
                    .collect();
                (topic.to_owned(), names)
            })
            .collect()
    }

    /// The epochs each of the producer ids `pids` was given by an InitProducerId the tape holds (the broker's answer), oldest
    /// first, at most five each: when and to which client. A producer the tape never saw get one is not in the answer.
    #[must_use]
    pub fn epochs(&self, pids: &[i64]) -> BTreeMap<i64, Vec<ProdEpoch>> {
        let inner = self.lock();
        let mut out: BTreeMap<i64, Vec<ProdEpoch>> = BTreeMap::new();
        for r in inner.ring.iter() {
            if r.api != ApiKey::InitProducerId || r.code != 0 {
                continue;
            }
            let Some((pid, epoch)) = r.facts.producer.filter(|(p, _)| pids.contains(p)) else {
                continue;
            };
            let list = out.entry(pid).or_default();
            if list.last().is_some_and(|e| e.epoch == epoch) {
                continue;
            }
            list.push(ProdEpoch {
                epoch,
                at_ms: r.last_ms,
                client: r.who.client.to_string(),
            });
            if list.len() > 5 {
                list.remove(0);
            }
        }
        out
    }

    /// The groups whose offsets the transaction `txn` of `producer` staged since `since_ms` (an AddOffsetsToTxn the broker
    /// took), as far as the tape reaches: DescribeTransactions does not list them.
    #[must_use]
    pub fn staged(&self, txn: &str, producer: (i64, i16), since_ms: i64) -> Vec<String> {
        let inner = self.lock();
        let mut out: Vec<String> = Vec::new();
        for r in inner.ring.iter() {
            let f = &r.facts;
            if r.api != ApiKey::AddOffsetsToTxn
                || r.code != 0
                || r.last_ms < since_ms
                || f.txn_id.as_deref() != Some(txn)
                || f.producer != Some(producer)
            {
                continue;
            }
            if let Some(g) = f.group.as_deref().filter(|g| !out.iter().any(|o| o == g)) {
                out.push(g.to_owned());
            }
        }
        out
    }

    /// Who wrote the abort marker of producer `pid` written at `at_ms`: the producer, if the tape holds its EndTxn abort
    /// within ten seconds of it; the broker, if the tape reaches back before the marker and holds none; `None` otherwise.
    #[must_use]
    pub fn abort_by(&self, pid: i64, at_ms: i64) -> Option<AbortBy> {
        const NEAR_MS: i64 = 10_000;
        let inner = self.lock();
        let oldest = inner.ring.iter().next()?.first_ms;
        let by = inner.ring.iter().find(|r| {
            r.api == ApiKey::EndTxn
                && r.code == 0
                && r.facts.producer.is_some_and(|(p, _)| p == pid)
                && r.facts.what.as_deref() == Some("abort")
                && (r.last_ms - at_ms).abs() <= NEAR_MS
        });
        match by {
            Some(r) => Some(AbortBy::Producer {
                client: r.who.client.to_string(),
            }),
            None if oldest < at_ms - NEAR_MS => Some(AbortBy::Broker),
            None => None,
        }
    }
}
