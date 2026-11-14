//! Rule 2, say it once: what is worth knowing but is not the answer (the registry is down and a cached copy was used;
//! a module this doze cannot run yet) is collected for the run, deduplicated, and printed once, last, one line each.
//! Under `--json` the same lines are the envelope's `warnings`.

/// The run's notices, in the order they were first raised.
#[derive(Debug, Default)]
pub struct Notices {
    lines: Vec<String>,
}

impl Notices {
    /// Raises a notice; the same words twice are said once.
    pub fn add(&mut self, line: impl Into<String>) {
        let line = line.into();
        if !self.lines.contains(&line) {
            self.lines.push(line);
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F2: the Go CLI printed one registry failure three to five times per command.
    #[test]
    fn the_same_notice_is_said_once() {
        let mut n = Notices::default();
        n.add("the module registry can't be reached");
        n.add("using cached valkey 0.2.1");
        n.add("the module registry can't be reached");
        assert_eq!(n.lines().len(), 2);
    }
}
