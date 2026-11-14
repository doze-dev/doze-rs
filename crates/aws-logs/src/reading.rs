//! Reading events: `GetLogEvents` and `FilterLogEvents`.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::{Core, VERSION};
use crate::model::{Event, Stream, stream_key};
use crate::pattern;
use crate::req::{Put, Req, invalid, no_group, no_stream};

/// Events one call returns at most (the API's own limit).
const MAX_EVENTS: usize = 10_000;
/// Rows read from the table at a time.
const CHUNK: usize = 200;

/// A row of the event table, taken apart.
#[derive(Debug)]
pub(crate) struct Row {
    /// The key after the stream's number: timestamp and sequence, as hex.
    pub(crate) pos: String,
    pub(crate) ts: i64,
    pub(crate) seq: u64,
    pub(crate) event: Event,
}

fn parse_row(key: &str, value: &[u8]) -> Option<Row> {
    let pos = key.get(8..)?.to_owned();
    let ts = i64::from_str_radix(pos.get(..16)?, 16).ok()?;
    let seq = u64::from_str_radix(pos.get(16..)?, 16).ok()?;
    let event = record::decode::<Event>("event", VERSION, value).ok()?;
    Some(Row {
        pos,
        ts,
        seq,
        event,
    })
}

impl Core {
    /// The rows of stream `no` in `from..to` (positions, as in a token), at most `want` of those `keep` accepts, read in chunks.
    /// `reverse` reads from the newest.
    pub(crate) fn rows(
        &self,
        no: u32,
        range: (&str, &str),
        want: usize,
        reverse: bool,
        keep: &dyn Fn(&Row) -> bool,
    ) -> Result<Vec<Row>, ApiError> {
        let (mut from, mut to) = (
            format!("{no:08x}{}", range.0),
            format!("{no:08x}{}", range.1),
        );
        let mut out = Vec::new();
        while out.len() < want {
            let chunk = self
                .events
                .scan(self.db.as_ref(), &from, &to, CHUNK, reverse)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let Some((edge, _)) = chunk.last() else {
                break;
            };
            let (edge, n) = (edge.clone(), chunk.len());
            for (key, value) in &chunk {
                if let Some(row) = parse_row(key, value).filter(|r| keep(r)) {
                    out.push(row);
                    if out.len() == want {
                        break;
                    }
                }
            }
            if n < CHUNK {
                break;
            }
            if reverse {
                to = edge;
            } else {
                from = format!("{edge}\u{0}");
            }
        }
        Ok(out)
    }

    /// The last `limit` rows of a range, oldest first.
    fn newest(&self, no: u32, range: (&str, &str), limit: usize) -> Result<Vec<Row>, ApiError> {
        let mut rows = self.rows(no, range, limit, true, &|_| true)?;
        rows.reverse();
        Ok(rows)
    }

    pub(crate) fn get_events(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = r.group_name().unwrap_or_default();
        if self.groups.get(&group).is_none() {
            return Err(no_group());
        }
        let stream = self
            .streams
            .get(&stream_key(&group, r.s("logStreamName")))
            .ok_or_else(no_stream)?
            .clone();
        self.sweep(&group);
        let limit = r
            .int("limit")
            .map_or(MAX_EVENTS, |n| usize::try_from(n).unwrap_or(MAX_EVENTS));
        let (start, end) = (r.int("startTime"), r.int("endTime"));
        let lo = hex_time(start.unwrap_or(0).max(0));
        let hi = end.map_or_else(|| "g".to_owned(), |e| hex_time(e.max(0)));
        let token = r.opt("nextToken");
        let everything = &|_: &Row| true;
        let rows = match token.map(split_token) {
            Some(Some((true, pos))) => self.rows(
                stream.no,
                (&format!("{pos}\u{0}"), &hi),
                limit,
                false,
                everything,
            )?,
            Some(Some((false, pos))) => self.newest(stream.no, (&lo, &pos), limit)?,
            Some(None) => return Err(invalid("The specified nextToken is invalid.")),
            None if r.flag("startFromHead") == Some(true) => {
                self.rows(stream.no, (&lo, &hi), limit, false, everything)?
            }
            None => self.newest(stream.no, (&lo, &hi), limit)?,
        };
        let (forward, backward) = tokens(&rows, token);
        Ok(json!({
            "events": rows.iter().map(|x| json!({
                "timestamp": x.ts,
                "message": x.event.message,
                "ingestionTime": x.event.ingested_ms,
            })).collect::<Vec<_>>(),
            "nextForwardToken": forward,
            "nextBackwardToken": backward,
        }))
    }

    pub(crate) fn filter_events(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = r.group_name().unwrap_or_default();
        if self.groups.get(&group).is_none() {
            return Err(no_group());
        }
        let pattern = pattern::parse(r.s("filterPattern")).map_err(crate::filters::refused)?;
        let streams = self.selected_streams(r, &group)?;
        self.sweep(&group);
        let limit = r
            .int("limit")
            .map_or(MAX_EVENTS, |n| usize::try_from(n).unwrap_or(MAX_EVENTS));
        let after = r.opt("nextToken").map(parse_filter_token).transpose()?;
        let lo_ts = r
            .int("startTime")
            .unwrap_or(0)
            .max(after.as_ref().map_or(0, |a| a.0));
        let lo = hex_time(lo_ts);
        let hi = r
            .int("endTime")
            .map_or_else(|| "g".to_owned(), |e| hex_time(e.max(0)));
        let mut found: Vec<(Row, &Stream)> = Vec::new();
        for s in &streams {
            let keep = |row: &Row| {
                pattern.matches(&row.event.message)
                    && after.as_ref().is_none_or(|(ts, name, seq)| {
                        (row.ts, s.name.as_str(), row.seq) > (*ts, name.as_str(), *seq)
                    })
            };
            for row in self.rows(s.no, (&lo, &hi), limit + 1, false, &keep)? {
                found.push((row, s));
            }
        }
        found.sort_by(|a, b| (a.0.ts, &a.1.name, a.0.seq).cmp(&(b.0.ts, &b.1.name, b.0.seq)));
        let more = found.len() > limit;
        found.truncate(limit);
        let mut out = json!({
            "events": found.iter().map(|(x, s)| json!({
                "logStreamName": s.name,
                "timestamp": x.ts,
                "message": x.event.message,
                "ingestionTime": x.event.ingested_ms,
                "eventId": format!("{:019}{:019}{:018}", s.no, x.ts, x.seq),
            })).collect::<Vec<_>>(),
        });
        if let (true, Some((x, s))) = (more, found.last()) {
            out.put(
                "nextToken",
                json!(format!("{:016x}:{:016x}:{}", x.ts, x.seq, s.name)),
            );
        }
        Ok(out)
    }

    /// The streams a filter reads: those named, those with the prefix, or all of the group's.
    fn selected_streams(&self, r: &Req<'_>, group: &str) -> Result<Vec<Stream>, ApiError> {
        let names = r.strings("logStreamNames");
        if !names.is_empty() {
            return names
                .iter()
                .map(|n| {
                    self.streams
                        .get(&stream_key(group, n))
                        .cloned()
                        .ok_or_else(no_stream)
                })
                .collect();
        }
        let prefix = r.opt("logStreamNamePrefix").unwrap_or_default();
        Ok(self
            .streams
            .with_prefix(&stream_key(group, prefix))
            .map(|(_, s)| s.clone())
            .collect())
    }
}

/// A time as the 16 hex digits a position starts with.
fn hex_time(ms: i64) -> String {
    format!("{ms:016x}")
}

/// `f/POS/s` or `b/POS/s` as (is forward, POS).
fn split_token(token: &str) -> Option<(bool, String)> {
    let (dir, rest) = token.split_once('/')?;
    let pos = rest.strip_suffix("/s")?;
    let valid = pos.len() == 32 && pos.bytes().all(|b| b.is_ascii_hexdigit());
    valid
        .then(|| (dir == "f", pos.to_owned()))
        .filter(|_| dir == "f" || dir == "b")
}

/// The tokens that continue a read: forward from the last event, backward from the first. With no events, the token the caller
/// gave comes back (and a first read of an empty stream gets the start).
fn tokens(rows: &[Row], given: Option<&str>) -> (String, String) {
    if let (Some(a), Some(b)) = (rows.first(), rows.last()) {
        return (format!("f/{}/s", b.pos), format!("b/{}/s", a.pos));
    }
    let pos = given
        .and_then(split_token)
        .map_or_else(|| "0".repeat(32), |(_, p)| p);
    (format!("f/{pos}/s"), format!("b/{pos}/s"))
}

/// A filter's page token: the time, sequence and stream of the last event of the page before.
fn parse_filter_token(token: &str) -> Result<(i64, String, u64), ApiError> {
    let bad = || invalid("The specified nextToken is invalid.");
    let mut parts = token.splitn(3, ':');
    let ts = i64::from_str_radix(parts.next().ok_or_else(bad)?, 16).map_err(|_| bad())?;
    let seq = u64::from_str_radix(parts.next().ok_or_else(bad)?, 16).map_err(|_| bad())?;
    let name = parts.next().ok_or_else(bad)?.to_owned();
    Ok((ts, name, seq))
}
