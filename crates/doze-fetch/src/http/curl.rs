//! The system's `curl` as the client (built without feature `tls`: the desktop app). macOS ships it at /usr/bin/curl and
//! Windows 10 and later in System32, so the app needs nothing installed; its own failures map to the same words the
//! in-process client says.

use super::{Failure, TOO_BIG, UNTRUSTED, dns_failure, timed_out};
use std::process::{Command, Stdio};
use std::time::Duration;

pub(super) fn get(url: &str, timeout: Duration, limit: u64) -> Result<Vec<u8>, Failure> {
    let out = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=http,https",
        ])
        .args(["--max-time", &format!("{:.3}", timeout.as_secs_f64())])
        .args(["--max-filesize", &limit.to_string()])
        .args(["--user-agent", concat!("doze/", env!("CARGO_PKG_VERSION"))])
        .args(["--write-out", "\n%{http_code}", "--", url])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                Failure::Unreachable("this machine has no `curl` to make the request".into())
            }
            _ => Failure::Unreachable(format!("`curl` could not be run ({e})")),
        })?;
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        let said = String::from_utf8_lossy(&out.stderr);
        return Err(Failure::Unreachable(words(code, said.trim(), timeout)));
    }
    answer(out.stdout, limit)
}

/// The body and status from curl's output: the body, a newline, then the status `--write-out` added.
fn answer(mut out: Vec<u8>, limit: u64) -> Result<Vec<u8>, Failure> {
    let cut = out.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let status = String::from_utf8_lossy(out.get(cut..).unwrap_or_default())
        .trim()
        .parse::<u16>()
        .unwrap_or(0);
    out.truncate(cut);
    match status {
        200 if out.len() as u64 > limit => Err(Failure::Unreachable(TOO_BIG.into())),
        200 => Ok(out),
        0 => Err(Failure::Unreachable("the answer was not HTTP".into())),
        s => Err(Failure::Status(s)),
    }
}

/// curl's exit code (`man curl`, EXIT CODES) in the words the in-process client uses.
fn words(code: i32, said: &str, timeout: Duration) -> String {
    match code {
        6 => "no DNS answer".into(),
        5 => format!("the proxy has no DNS answer ({said})"),
        7 if said.to_ascii_lowercase().contains("refused") => "the connection was refused".into(),
        7 | 52 | 55 | 56 => format!("the connection failed ({said})"),
        28 => timed_out(timeout),
        35 | 51 | 53 | 54 | 58 | 59 | 60 | 66 | 77 | 80 | 82 | 83 | 90 | 91 => UNTRUSTED.into(),
        63 => TOO_BIG.into(),
        _ if dns_failure(said) => "no DNS answer".into(),
        _ => format!("the request failed (curl exit {code}: {said})"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_is_split_from_the_body() {
        assert_eq!(answer(b"a\nb\n200".to_vec(), 16), Ok(b"a\nb".to_vec()));
        assert_eq!(answer(b"\n404".to_vec(), 16), Err(Failure::Status(404)));
        assert!(matches!(
            answer(b"nothing".to_vec(), 16),
            Err(Failure::Unreachable(_))
        ));
    }

    #[test]
    fn exit_codes_say_what_the_in_process_client_says() {
        let t = Duration::from_secs(3);
        assert_eq!(words(6, "Could not resolve host: x", t), "no DNS answer");
        assert_eq!(
            words(7, "Failed to connect: Connection refused", t),
            "the connection was refused"
        );
        assert_eq!(words(28, "Operation timed out", t), "no answer within 3s");
        assert_eq!(words(60, "SSL certificate problem", t), UNTRUSTED);
        assert_eq!(
            words(63, "Exceeded the maximum allowed file size", t),
            TOO_BIG
        );
        assert_eq!(
            words(99, "odd", t),
            "the request failed (curl exit 99: odd)"
        );
    }

    /// The system's curl against a local server, whichever client the build selects.
    #[test]
    fn curl_itself_fetches_and_reports_status() {
        let ok = super::super::tests_support::serve("200 OK", b"x: 1\n");
        assert_eq!(
            get(&ok, Duration::from_secs(5), super::super::LIMIT).unwrap(),
            b"x: 1\n"
        );
        let gone = super::super::tests_support::serve("410 Gone", b"");
        assert_eq!(
            get(&gone, Duration::from_secs(5), super::super::LIMIT),
            Err(Failure::Status(410))
        );
    }
}
