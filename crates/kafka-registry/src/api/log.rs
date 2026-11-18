//! The request log: the newest [`MAX_REQUESTS`] requests, in memory, for the console's strip of what clients asked (the
//! broker's traffic recorder sees only Kafka requests). It keeps what was asked and what came of it, never a body. The
//! console's own polling (marked with [`OWN_HEADER`]) and the `/_doze/` routes are not kept.

use std::collections::VecDeque;
use std::time::Duration;

use serde::Serialize;

/// How many requests are kept.
pub const MAX_REQUESTS: usize = 2000;
/// A path (with its query) past this is kept cut short.
pub const MAX_REQUEST_URL: usize = 512;
/// The header that marks a request as the console's own.
pub const OWN_HEADER: &str = "x-doze-own";

/// One request the registry answered.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedRequest {
    pub seq: u64,
    pub at: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    #[serde(rename = "tookNanos")]
    pub took_nanos: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent: String,
    /// The subject the path named.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub subject: String,
    /// The schema id the path named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
}

impl LoggedRequest {
    pub(crate) fn took(&mut self, d: Duration) {
        self.took_nanos = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    }
}

/// The ring.
#[derive(Debug, Default)]
pub struct RequestLog {
    buf: VecDeque<LoggedRequest>,
    seq: u64,
}

/// A page of requests, newest first, and the cursor for the next.
#[derive(Debug, Clone, Serialize)]
pub struct RequestPage {
    pub requests: Vec<LoggedRequest>,
    /// Pass as `before` for older ones; absent when there are none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<u64>,
    /// How many the log holds, of at most `max`.
    pub kept: usize,
    pub max: usize,
}

impl RequestLog {
    pub fn add(&mut self, mut r: LoggedRequest) {
        self.seq += 1;
        r.seq = self.seq;
        if self.buf.len() == MAX_REQUESTS {
            self.buf.pop_front();
        }
        self.buf.push_back(r);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// The requests for a subject (a request by id counts for each subject holding the id: `ids`) older than `before`,
    /// newest first, at most `limit` (100 when not positive, at most [`MAX_REQUESTS`]).
    #[must_use]
    pub fn page(&self, subject: &str, ids: &[u32], before: u64, limit: i64) -> RequestPage {
        let limit = match usize::try_from(limit) {
            Ok(0) | Err(_) => 100,
            Ok(n) => n.min(MAX_REQUESTS),
        };
        let mut page = RequestPage {
            requests: Vec::new(),
            next: None,
            kept: self.buf.len(),
            max: MAX_REQUESTS,
        };
        let wanted = self
            .buf
            .iter()
            .rev()
            .filter(|r| before == 0 || r.seq < before)
            .filter(|r| {
                subject.is_empty()
                    || r.subject == subject
                    || r.id
                        .is_some_and(|id| u32::try_from(id).is_ok_and(|id| ids.contains(&id)))
            });
        for r in wanted {
            if page.requests.len() == limit {
                page.next = page.requests.last().map(|l| l.seq);
                break;
            }
            page.requests.push(r.clone());
        }
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(subject: &str, id: Option<i64>) -> LoggedRequest {
        LoggedRequest {
            seq: 0,
            at: String::new(),
            method: "GET".into(),
            path: "/".into(),
            status: 200,
            took_nanos: 0,
            user: String::new(),
            from: String::new(),
            agent: String::new(),
            subject: subject.into(),
            id,
        }
    }

    #[test]
    fn the_ring_holds_exactly_its_bound_and_pages_newest_first() {
        let mut log = RequestLog::default();
        for _ in 0..MAX_REQUESTS + 5 {
            log.add(req("a", None));
        }
        assert_eq!(log.len(), MAX_REQUESTS);
        let p = log.page("", &[], 0, 2);
        assert_eq!(
            p.requests.iter().map(|r| r.seq).collect::<Vec<_>>(),
            [2005, 2004]
        );
        assert_eq!(p.next, Some(2004));
        assert_eq!(
            log.page("", &[], 2004, 1).requests.first().map(|r| r.seq),
            Some(2003)
        );
    }

    #[test]
    fn a_request_by_id_counts_for_the_subjects_holding_it() {
        let mut log = RequestLog::default();
        log.add(req("x", None));
        log.add(req("", Some(7)));
        log.add(req("y", None));
        let p = log.page("x", &[7], 0, 0);
        assert_eq!(p.requests.iter().map(|r| r.seq).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(p.next, None);
    }
}
