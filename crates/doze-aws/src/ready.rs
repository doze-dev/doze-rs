//! The block a person reads once doze-aws listens, on standard output (the log, on standard error, keeps its own lines: a supervisor
//! and the conformance harness wait for `doze-aws listening on ...` there). `doze-aws 2>/dev/null` shows just this.

use std::time::Duration;

use crate::config::{Config, display_path};
use crate::env::Shell;
use crate::services::{ALL, Svc};
use crate::style::Style;

/// What the block says.
#[derive(Debug)]
pub(crate) struct Ready<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) services: &'a [&'static Svc],
    pub(crate) region: &'a str,
    pub(crate) account: &'a str,
    pub(crate) took: Duration,
    pub(crate) config: &'a Config,
    pub(crate) shell: Shell,
}

impl Ready<'_> {
    /// The block's text.
    pub(crate) fn render(&self, s: Style) -> String {
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut rows: Vec<(&str, String)> = vec![
            ("endpoint", s.bold(self.endpoint)),
            ("serving", self.serving()),
            ("data", display_path(&self.config.data_dir(), &cwd)),
        ];
        if let Some(f) = &self.config.file {
            rows.push(("config", display_path(f, &cwd)));
        }
        for o in self.config.overrules() {
            let said = o
                .overruled
                .as_ref()
                .map(|v| v.join(","))
                .unwrap_or_default();
            let by = crate::config::describe(o.key, &o.source, None);
            rows.push((
                "overrides",
                format!("{by} over the file's {} = {said}", o.key.name),
            ));
        }
        let ms = self.took.as_millis();
        let mut out = format!(
            "\n  {} {}\n\n",
            s.bold("doze-aws is up"),
            s.dim(&format!("in {ms} ms"))
        );
        for (label, value) in rows {
            out.push_str(&format!("    {}  {value}\n", s.dim(&format!("{label:<9}"))));
        }
        let hint = self.shell.hint();
        let width = hint.len().max(15);
        out.push('\n');
        out.push_str(&format!(
            "    {}  {}\n",
            s.cyan(&format!("{hint:<width$}")),
            s.dim("point this shell at it")
        ));
        out.push_str(&format!(
            "    {}  {}\n\n",
            s.cyan(&format!("{:<width$}", "doze-aws doctor")),
            s.dim("when something looks wrong")
        ));
        out
    }

    fn serving(&self) -> String {
        let n = self.services.len();
        let what = match n {
            0 => "no services".to_owned(),
            1..=4 => {
                let names: Vec<&str> = self.services.iter().map(|s| s.name).collect();
                match names.split_last() {
                    Some((last, [])) => (*last).to_owned(),
                    Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
                    None => String::new(),
                }
            }
            n => format!("{n} services"),
        };
        let more = ALL.len() - n;
        let rest = if more > 0 {
            format!(" ({more} more with --services all)")
        } else {
            String::new()
        };
        format!("{what} in {}, account {}{rest}", self.region, self.account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Given;

    #[test]
    fn the_block_names_the_endpoint_the_services_and_what_to_run() {
        let dir = std::env::temp_dir();
        let config = Config::load(&Given::new(), None, &dir).unwrap();
        let few: Vec<&Svc> = ALL
            .iter()
            .filter(|s| s.name == "sqs" || s.name == "s3")
            .collect();
        let ready = Ready {
            endpoint: "http://127.0.0.1:4566",
            services: &few,
            region: "us-east-1",
            account: "000000000000",
            took: Duration::from_millis(12),
            config: &config,
            shell: Shell::Sh,
        };
        let text = ready.render(Style::plain());
        assert!(text.contains("doze-aws is up in 12 ms"), "{text}");
        assert!(text.contains("endpoint   http://127.0.0.1:4566"), "{text}");
        assert!(
            text.contains(
                "s3 and sqs in us-east-1, account 000000000000 (16 more with --services all)"
            ),
            "{text}"
        );
        assert!(
            text.contains("eval \"$(doze-aws env)\"  point this shell at it"),
            "{text}"
        );
        assert!(
            text.contains("doze-aws doctor         when something looks wrong"),
            "{text}"
        );
        assert!(!text.contains('\x1b'));
    }
}
