//! Following the daemon: `doze logs -f` and `doze status -w` print as things happen, until the stack stops or the
//! person presses Ctrl-C (which ends a follow the way it is meant to end: exit 0). With `--json` the output is NDJSON
//! as Go writes it: one `{"type":…}` object per line, the last one `{"type":"end","ok":…}`, and no envelope after.

use crate::diag::Diagnostic;
use crate::stack::daemon_error;
use doze_control::{Frame, Stream, StreamEvent};
use doze_os::interrupt::Interrupt;
use serde::Serialize;
use std::io::{self, Write};
use std::time::Duration;

/// How often a follow looks at the interrupt while no frame comes.
const TICK: Duration = Duration::from_millis(100);

/// How a follow ended.
#[derive(Debug)]
pub(crate) enum Ended {
    /// The daemon ended the stream, or the person pressed Ctrl-C.
    Done,
    /// The connection went: the daemon stopped, or crashed.
    Lost(Diagnostic),
}

/// Hands each frame of `stream` to `on` until the stream ends, the connection goes, or an interrupt comes. A frame
/// `on` cannot write (a reader that went away, `| head`) ends the follow too.
pub(crate) fn follow(
    stream: &Stream,
    interrupt: &Interrupt,
    mut on: impl FnMut(Frame) -> io::Result<()>,
) -> Ended {
    loop {
        if interrupt.raised() {
            return Ended::Done;
        }
        match stream.next_timeout(TICK) {
            None => {}
            Some(StreamEvent::Frame(f)) => {
                if on(*f).is_err() {
                    return Ended::Done;
                }
            }
            Some(StreamEvent::End(None)) => return Ended::Done,
            Some(StreamEvent::End(Some(e))) | Some(StreamEvent::Lost(e)) => {
                return Ended::Lost(daemon_error(&doze_control::Error::Closed(e), None));
            }
        }
    }
}

/// One NDJSON line: `{"type": typ, …the fields of v}`, as Go's `writeStream`.
pub(crate) fn line(out: &mut dyn Write, typ: &str, v: &impl Serialize) -> io::Result<()> {
    let body = doze_control::go_line(v).map_err(io::Error::other)?;
    let ty = doze_control::go_line(typ).map_err(io::Error::other)?;
    let rest = body.strip_prefix('{').unwrap_or(&body);
    if rest == "}" || body == "null" {
        writeln!(out, "{{\"type\":{ty}}}")?;
    } else {
        writeln!(out, "{{\"type\":{ty},{rest}")?;
    }
    out.flush()
}

/// The last line: how the stream ended.
pub(crate) fn end(out: &mut dyn Write, problem: Option<&Diagnostic>) -> io::Result<()> {
    #[derive(Serialize)]
    struct End<'a> {
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<crate::diag::ErrorBody<'a>>,
    }
    line(
        out,
        "end",
        &End {
            ok: problem.is_none(),
            error: problem.map(Diagnostic::json),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_carry_their_type_first() {
        let mut out = Vec::new();
        line(&mut out, "log", &serde_json::json!({"line": "a<b"})).unwrap();
        end(&mut out, None).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"type\":\"log\",\"line\":\"a\\u003cb\"}\n{\"type\":\"end\",\"ok\":true}\n"
        );
    }
}
