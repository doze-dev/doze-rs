//! The connection table: the connections that are open now, who each is, and what it has used. Kept only while an observer is
//! installed (the server hands out connection numbers only then), at most [`MOST`] entries, and emptied as connections close.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;

use kafka_wire::messages::ApiKey;

use super::clip;
use super::row::Who;

/// The most connections tracked at once (the server's own limit is 2,048).
pub const MOST: usize = 4096;

/// One open connection.
#[derive(Debug)]
pub struct Conn {
    pub opened_ms: i64,
    pub last_ms: i64,
    pub who: Arc<Who>,
    pub requests: u64,
    pub errors: u64,
    /// By API key: the highest version used and how many requests.
    pub apis: BTreeMap<i16, (i16, u64)>,
}

/// The table.
#[derive(Debug, Default)]
pub struct Conns {
    map: HashMap<u64, Conn>,
}

impl Conns {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Conn)> {
        self.map.iter()
    }

    pub fn open(&mut self, id: u64, peer: Option<SocketAddr>, now_ms: i64) {
        if self.map.len() >= MOST {
            return;
        }
        let who = Who {
            client: "".into(),
            software: None,
            version: None,
            principal: "".into(),
            peer: peer.map(|p| p.to_string().into()),
        };
        self.map.insert(
            id,
            Conn {
                opened_ms: now_ms,
                last_ms: now_ms,
                who: Arc::new(who),
                requests: 0,
                errors: 0,
                apis: BTreeMap::new(),
            },
        );
    }

    pub fn close(&mut self, id: u64) {
        self.map.remove(&id);
        if self.map.is_empty() {
            self.map = HashMap::new();
        }
    }

    /// What this connection is, as of this request: the same `Who` while nothing changed (no allocation), a new one when the
    /// client id, the principal or the software did. A connection the table does not hold (past the cap) gets one made on the spot.
    pub fn who(
        &mut self,
        id: u64,
        client: &str,
        principal: &str,
        software: Option<(Box<str>, Box<str>)>,
    ) -> Arc<Who> {
        let Some(conn) = self.map.get_mut(&id) else {
            return Arc::new(Who {
                client: clip::name(client),
                software: software.as_ref().map(|s| s.0.clone()),
                version: software.map(|s| s.1),
                principal: clip::name(principal),
                peer: None,
            });
        };
        let w = &conn.who;
        let same = *w.client == *client
            && *w.principal == *principal
            && software
                .as_ref()
                .is_none_or(|s| w.software.as_deref() == Some(&*s.0));
        if same {
            return Arc::clone(&conn.who);
        }
        let (name, version) = match software {
            Some((n, v)) => (Some(n), Some(v)),
            None => (w.software.clone(), w.version.clone()),
        };
        let fresh = Arc::new(Who {
            client: clip::name(client),
            software: name,
            version,
            principal: clip::name(principal),
            peer: w.peer.clone(),
        });
        conn.who = Arc::clone(&fresh);
        fresh
    }

    /// A request ended on this connection.
    pub fn note(&mut self, id: u64, api: ApiKey, version: i16, errored: bool, now_ms: i64) {
        if let Some(c) = self.map.get_mut(&id) {
            c.last_ms = now_ms;
            c.requests = c.requests.saturating_add(1);
            c.errors += u64::from(errored);
            let e = c.apis.entry(api as i16).or_insert((version, 0));
            e.0 = e.0.max(version);
            e.1 = e.1.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests;
