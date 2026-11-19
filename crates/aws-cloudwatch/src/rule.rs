//! A composite alarm's `AlarmRule`, read and worked out. The grammar is the model's documentation of `PutCompositeAlarm`: the
//! functions `ALARM(name)`, `OK(name)` and `INSUFFICIENT_DATA(name)` (a name or an ARN, double-quoted or not), `TRUE`, `FALSE`,
//! `AND`, `OR`, `NOT` and parentheses; "as many as 100 children alarms" and "as many as 500 elements". `NOT` binds tighter than `AND`,
//! and `AND` tighter than `OR` (memory). Fuzzed by `fuzz_alarm_rule`.
// fuzz:parser fuzz_alarm_rule

/// The most alarms a rule names (the model).
const MAX_CHILDREN: usize = 100;
/// The most elements a rule has: alarms, `TRUE`/`FALSE`, parentheses (the model).
const MAX_ELEMENTS: usize = 500;
/// How deep parentheses and `NOT`s nest before the rule is refused (ours: the element bound alone allows 250).
const MAX_DEPTH: usize = 64;

/// A rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rule {
    Const(bool),
    /// The alarm (its name, an ARN read down to the name) is in this state.
    In(&'static str, String),
    Not(Box<Rule>),
    And(Box<Rule>, Box<Rule>),
    Or(Box<Rule>, Box<Rule>),
}

impl Rule {
    /// Whether the rule holds, given each alarm's state (`None`: no such alarm, which is in no state).
    pub(crate) fn holds(&self, state_of: &dyn Fn(&str) -> Option<String>) -> bool {
        match self {
            Self::Const(b) => *b,
            Self::In(state, name) => state_of(name).is_some_and(|s| s == *state),
            Self::Not(r) => !r.holds(state_of),
            Self::And(a, b) => a.holds(state_of) && b.holds(state_of),
            Self::Or(a, b) => a.holds(state_of) || b.holds(state_of),
        }
    }

    /// The alarms the rule names, each once.
    pub(crate) fn children(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<String>) {
        match self {
            Self::Const(_) => {}
            Self::In(_, name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Self::Not(r) => r.collect(out),
            Self::And(a, b) | Self::Or(a, b) => {
                a.collect(out);
                b.collect(out);
            }
        }
    }
}

/// The name an alarm is called by in a rule: an ARN (`arn:aws:cloudwatch:r:a:alarm:NAME`) read down to its name.
fn alarm_name(text: &str) -> String {
    match text.split_once(":alarm:") {
        Some((head, name)) if head.starts_with("arn:") => name.to_owned(),
        _ => text.to_owned(),
    }
}

/// Reads a rule, or says why it is not one. Wording (memory).
pub(crate) fn parse(text: &str) -> Result<Rule, String> {
    let mut p = Parser {
        text,
        at: 0,
        depth: 0,
        elements: 0,
    };
    let rule = p.or()?;
    p.skip_space();
    if p.at < text.len() {
        return Err(p.unexpected());
    }
    if rule.children().len() > MAX_CHILDREN {
        return Err(format!("AlarmRule can name at most {MAX_CHILDREN} alarms."));
    }
    Ok(rule)
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
    elements: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start().len();
    }

    fn unexpected(&self) -> String {
        let near: String = self.rest().chars().take(20).collect();
        if near.is_empty() {
            "AlarmRule ended where an alarm state function, TRUE, FALSE or a parenthesis was expected.".to_owned()
        } else {
            format!("AlarmRule could not be read near '{near}'.")
        }
    }

    /// The next word (letters and underscores), upper-cased, without taking it.
    fn word(&self) -> Option<(String, usize)> {
        let rest = self.rest().trim_start();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphabetic() || c == '_'))
            .unwrap_or(rest.len());
        let lead = self.rest().len() - rest.len();
        (len > 0).then(|| {
            (
                rest.get(..len).unwrap_or_default().to_ascii_uppercase(),
                lead + len,
            )
        })
    }

    fn take_word(&mut self, want: &str) -> bool {
        match self.word() {
            Some((w, len)) if w == want => {
                self.at += len;
                true
            }
            _ => false,
        }
    }

    fn take_char(&mut self, c: char) -> bool {
        self.skip_space();
        if self.rest().starts_with(c) {
            self.at += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn element(&mut self) -> Result<(), String> {
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(format!(
                "AlarmRule can have at most {MAX_ELEMENTS} elements."
            ));
        }
        Ok(())
    }

    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("AlarmRule nests deeper than {MAX_DEPTH} levels."));
        }
        Ok(())
    }

    fn or(&mut self) -> Result<Rule, String> {
        let mut left = self.and()?;
        while self.take_word("OR") {
            left = Rule::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Rule, String> {
        let mut left = self.unary()?;
        while self.take_word("AND") {
            left = Rule::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Rule, String> {
        if self.take_word("NOT") {
            self.deeper()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Rule::Not(Box::new(inner)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Rule, String> {
        if self.take_char('(') {
            self.element()?;
            self.deeper()?;
            let inner = self.or()?;
            self.depth -= 1;
            if !self.take_char(')') {
                return Err(self.unexpected());
            }
            return Ok(inner);
        }
        let Some((word, len)) = self.word() else {
            return Err(self.unexpected());
        };
        let state = match word.as_str() {
            "TRUE" | "FALSE" => {
                self.at += len;
                self.element()?;
                return Ok(Rule::Const(word == "TRUE"));
            }
            "ALARM" => "ALARM",
            "OK" => "OK",
            "INSUFFICIENT_DATA" => "INSUFFICIENT_DATA",
            _ => return Err(self.unexpected()),
        };
        self.at += len;
        if !self.take_char('(') {
            return Err(format!(
                "{state} must be followed by an alarm name in parentheses."
            ));
        }
        let name = self.name()?;
        self.element()?;
        Ok(Rule::In(state, alarm_name(&name)))
    }

    /// An alarm name up to its closing parenthesis: in double quotes, or bare and trimmed.
    fn name(&mut self) -> Result<String, String> {
        self.skip_space();
        let rest = self.rest();
        let (name, used) = if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted
                .find('"')
                .ok_or_else(|| "AlarmRule has an alarm name with no closing quote.".to_owned())?;
            (quoted.get(..end).unwrap_or_default().to_owned(), end + 2)
        } else {
            let end = rest.find(')').unwrap_or(rest.len());
            (rest.get(..end).unwrap_or_default().trim().to_owned(), end)
        };
        self.at += used;
        if name.is_empty() || !self.take_char(')') {
            return Err("AlarmRule has an alarm state function without an alarm name.".to_owned());
        }
        Ok(name)
    }
}

/// Whether `text` reads as a rule. Exists so the parser can be fuzzed.
pub(crate) fn check(text: &str) -> bool {
    parse(text).is_ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn states(name: &str) -> Option<String> {
        match name {
            "cpu" | "disk" => Some("ALARM".to_owned()),
            "net" => Some("OK".to_owned()),
            "new" => Some("INSUFFICIENT_DATA".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn the_models_examples_read_and_hold_as_documented() {
        for (rule, holds) in [
            ("ALARM(cpu) AND ALARM(disk)", true),
            ("ALARM(cpu) AND NOT ALARM(net)", true),
            ("(ALARM(cpu) OR ALARM(gone)) AND OK(net)", true),
            ("ALARM(\"cpu\") AND OK(\"net\")", true),
            ("INSUFFICIENT_DATA(new)", true),
            ("TRUE", true),
            ("FALSE OR NOT TRUE", false),
            ("ALARM(arn:aws:cloudwatch:us-east-1:1:alarm:cpu)", true),
            ("ALARM(gone)", false),
            // NOT before AND before OR.
            ("NOT ALARM(net) AND ALARM(cpu) OR FALSE", true),
            ("ALARM(net) OR ALARM(cpu) AND FALSE", false),
        ] {
            assert_eq!(parse(rule).unwrap().holds(&states), holds, "{rule}");
        }
        assert_eq!(
            parse("ALARM(a) OR OK(b) AND ALARM(a)").unwrap().children(),
            ["a", "b"]
        );
    }

    #[test]
    fn what_is_not_a_rule_is_refused() {
        for bad in [
            "",
            "ALARM",
            "ALARM()",
            "ALARM(cpu",
            "(ALARM(cpu)",
            "ALARM(cpu) AND",
            "SOMETIMES(cpu)",
            "ALARM(\"cpu)",
            "ALARM(cpu) ALARM(disk)",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = format!("{}TRUE{}", "(".repeat(100), ")".repeat(100));
        assert!(parse(&deep).is_err());
        let wide = vec!["TRUE"; 501].join(" OR ");
        assert!(parse(&wide).unwrap_err().contains("500"));
        let many: Vec<String> = (0..101).map(|i| format!("ALARM(a{i})")).collect();
        assert!(parse(&many.join(" OR ")).unwrap_err().contains("100"));
    }
}
