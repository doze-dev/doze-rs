//! What the engine says about itself, for the console: `INFO`'s fields, and the lines `MONITOR` streams. Both are
//! read from the engine, so neither is trusted to be well formed: anything unexpected is skipped, never a panic.

use std::collections::BTreeMap;

/// `INFO`'s `key:value` lines, flattened (a key appears in one section only). Section headers, blank lines and lines
/// without a colon are skipped.
// fuzz:parser fuzz_text
pub fn parse_info(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A compound `INFO` value such as a keyspace line's `keys=12,expires=3,avg_ttl=0`, as its fields.
// fuzz:parser fuzz_text
pub fn fields(value: &str) -> BTreeMap<String, String> {
    value
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// One command a client sent, as `MONITOR` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitored {
    /// Seconds since the epoch, as the server wrote them (`1700000000.123456`).
    pub time: String,
    /// The database and the client's address (`0 127.0.0.1:5000`, `0 unix:/…`).
    pub client: String,
    /// The command and its arguments, quoted as the server quotes them.
    pub command: String,
}

/// A `MONITOR` line (`1700000000.123456 [0 127.0.0.1:5000] "SET" "k" "v"`), or `None` for anything else (the `OK`
/// that starts the stream).
// fuzz:parser fuzz_text
pub fn parse_monitor(line: &str) -> Option<Monitored> {
    let (time, rest) = line.split_once(' ')?;
    let rest = rest.strip_prefix('[')?;
    let (client, command) = rest.split_once("] ")?;
    if time.is_empty() || !time.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some(Monitored {
        time: time.to_string(),
        client: client.to_string(),
        command: command.to_string(),
    })
}

/// Bytes as a person reads them, in the 1024-based units valkey's own `256mb` uses: `512 B`, `1.5 KB`, `256 MB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 || value.fract() < 0.05 {
        format!("{value:.0} {name}")
    } else {
        format!("{value:.1} {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_flattens_sections_and_skips_what_it_cannot_read() {
        let text = "# Server\r\nvalkey_version:9.1.1\r\n\r\n# Memory\r\nused_memory:1048576\r\nnot a field\r\n\
                    # Keyspace\r\ndb0:keys=12,expires=3,avg_ttl=0\r\n";
        let info = parse_info(text);
        assert_eq!(
            info.get("valkey_version").map(String::as_str),
            Some("9.1.1")
        );
        assert_eq!(info.get("used_memory").map(String::as_str), Some("1048576"));
        assert_eq!(info.len(), 3);
        let db0 = fields(info.get("db0").map(String::as_str).unwrap_or_default());
        assert_eq!(db0.get("keys").map(String::as_str), Some("12"));
        assert_eq!(db0.get("expires").map(String::as_str), Some("3"));
    }

    #[test]
    fn monitor_lines_split_into_time_client_and_command() {
        assert_eq!(
            parse_monitor(r#"1700000000.123456 [0 127.0.0.1:5000] "SET" "k" "v v""#),
            Some(Monitored {
                time: "1700000000.123456".into(),
                client: "0 127.0.0.1:5000".into(),
                command: r#""SET" "k" "v v""#.into(),
            })
        );
        assert_eq!(parse_monitor("OK"), None);
        assert_eq!(parse_monitor("x [0 a] b"), None);
    }

    #[test]
    fn bytes_read_like_the_units_people_write() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(256 << 20), "256 MB");
        assert_eq!(human_bytes(3 << 30), "3 GB");
    }
}
