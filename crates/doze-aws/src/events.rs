//! Following a stack's events as they happen: a table that grows a line per event (time since the command started, logical id,
//! type, status, reason), until the stack settles; then the verdict, with the cause of a failure picked out.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::cfn::{Cfn, CfnError, Event};
use crate::style::{Style, pad};

/// How often the stack is asked for news (the client polls; the server keeps no timer for it).
const POLL: Duration = Duration::from_millis(200);

/// A stack being followed.
#[derive(Debug)]
pub(crate) struct Follow {
    name: String,
    seen: HashSet<String>,
    started_ms: i64,
    clock: Instant,
    widths: [usize; 3],
    style: Style,
    failure: Option<Event>,
}

/// How a stack settled.
#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) status: String,
    pub(crate) reason: String,
    pub(crate) failure: Option<Event>,
    pub(crate) took: Duration,
}

impl Follow {
    /// Starts following `name` (its events so far are not shown) and prints the table's head.
    pub(crate) fn start(cfn: &Cfn, name: &str, style: Style) -> Result<Self, CfnError> {
        let seen = match cfn.events(name) {
            Ok(events) => events.into_iter().map(|e| e.id).collect(),
            Err(CfnError::Api { .. }) => HashSet::new(),
            Err(e) => return Err(e),
        };
        let widths = [name.len().max(20), 26, 35];
        let head = format!(
            "    {}  {}  {}  {}  {}",
            pad("time", 6),
            pad("logical id", widths[0]),
            pad("type", widths[1]),
            pad("status", widths[2]),
            "reason"
        );
        println!("{}", style.dim(&head));
        Ok(Self {
            name: name.to_owned(),
            seen,
            started_ms: now_ms(),
            clock: Instant::now(),
            widths,
            style,
            failure: None,
        })
    }

    /// Prints events until the stack is no longer in progress.
    pub(crate) fn until_settled(mut self, cfn: &Cfn) -> Result<Outcome, CfnError> {
        loop {
            self.show(cfn)?;
            let stack = cfn.stack(&self.name)?;
            let (status, reason) = stack.map_or_else(
                || ("DELETE_COMPLETE".to_owned(), String::new()),
                |s| (s.status, s.reason),
            );
            if !status.ends_with("_IN_PROGRESS") {
                self.show(cfn)?;
                return Ok(Outcome {
                    status,
                    reason,
                    failure: self.failure,
                    took: self.clock.elapsed(),
                });
            }
            std::thread::sleep(POLL);
        }
    }

    fn show(&mut self, cfn: &Cfn) -> Result<(), CfnError> {
        let events = cfn.events(&self.name)?;
        for e in events.into_iter().rev() {
            if self.seen.insert(e.id.clone()) {
                self.line(&e);
                let failed = e.status.ends_with("_FAILED") && !e.reason.contains("cancelled");
                let better = self
                    .failure
                    .as_ref()
                    .is_none_or(|f| f.kind == STACK && e.kind != STACK);
                if failed && better {
                    self.failure = Some(e);
                }
            }
        }
        Ok(())
    }

    fn line(&mut self, e: &Event) {
        self.widths[0] = self.widths[0].max(e.logical.len());
        self.widths[1] = self.widths[1].max(e.kind.len());
        self.widths[2] = self.widths[2].max(e.status.len());
        let s = self.style;
        let secs = (ms_of(&e.time) - self.started_ms).max(0);
        let time = format!("{}.{}s", secs / 1000, (secs % 1000) / 100);
        let status = pad(&e.status, self.widths[2]);
        println!(
            "    {}  {}  {}  {}  {}",
            s.dim(&pad(&time, 6)),
            pad(&e.logical, self.widths[0]),
            s.dim(&pad(&e.kind, self.widths[1])),
            color_status(s, &e.status, &status),
            s.dim(&e.reason)
        );
    }
}

const STACK: &str = "AWS::CloudFormation::Stack";

fn color_status(s: Style, status: &str, padded: &str) -> String {
    if status.ends_with("_FAILED") {
        s.red(padded)
    } else if status.contains("ROLLBACK") {
        s.yellow(padded)
    } else if status.ends_with("_COMPLETE") {
        s.green(padded)
    } else {
        padded.to_owned()
    }
}

impl Outcome {
    /// Prints the verdict and the stack's outputs; the exit code (non-zero unless the create or update completed).
    pub(crate) fn report(&self, cfn: &Cfn, name: &str, style: Style) -> i32 {
        let took = format!("{:.1}s", self.took.as_secs_f64());
        if self.status == "CREATE_COMPLETE" || self.status == "UPDATE_COMPLETE" {
            println!(
                "\n  {} {} {} in {took}",
                style.green("✓"),
                style.bold(name),
                self.status
            );
            if let Ok(Some(stack)) = cfn.stack(name) {
                print_outputs(&stack.outputs, style);
            }
            println!();
            return 0;
        }
        println!(
            "\n  {} {} {} after {took}",
            style.red("✗"),
            style.bold(name),
            style.red(&self.status)
        );
        if let Some(f) = &self.failure {
            println!(
                "    {} {} ({}): {}",
                style.bold("cause"),
                f.logical,
                f.kind,
                style.red(&f.reason)
            );
        } else if !self.reason.is_empty() {
            println!("    {} {}", style.bold("cause"), style.red(&self.reason));
        }
        println!();
        1
    }
}

fn print_outputs(outputs: &[(String, String)], style: Style) {
    if outputs.is_empty() {
        return;
    }
    let width = outputs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    println!("\n    {}", style.dim("outputs"));
    for (k, v) in outputs {
        println!("    {}  {v}", pad(k, width));
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// An event's time in milliseconds since 1970 (`2026-11-14T10:00:00.250Z`).
pub(crate) fn ms_of(ts: &str) -> i64 {
    let secs = aws_core::time::parse_rfc3339(ts).unwrap_or(0);
    let frac = ts
        .split_once('.')
        .map(|(_, f)| {
            f.chars()
                .take_while(char::is_ascii_digit)
                .chain("000".chars())
                .take(3)
                .collect::<String>()
        })
        .and_then(|f| f.parse::<i64>().ok())
        .unwrap_or(0);
    secs * 1000 + frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_times_keep_their_milliseconds() {
        assert_eq!(ms_of("1970-01-01T00:00:01.25Z"), 1250);
        assert_eq!(ms_of("1970-01-01T00:00:02Z"), 2000);
    }
}
