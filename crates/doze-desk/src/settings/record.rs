//! Hands for tests: they only record what they were asked, and every process they stop is gone at once. Nothing
//! runs on the machine.

use std::path::Path;

use super::repair::{Fail, Hands};

#[derive(Debug, Default)]
pub(crate) struct Record {
    pub(crate) asked: Vec<String>,
    /// The administrator prompt is closed.
    pub(crate) cancel: bool,
    /// A doze command fails, saying this.
    pub(crate) fail: Option<String>,
}

impl Hands for Record {
    fn doze(&mut self, dir: Option<&Path>, args: &[String]) -> Result<(), Fail> {
        let at = dir
            .map(|d| format!(" in {}", d.display()))
            .unwrap_or_default();
        self.asked.push(format!("doze {}{at}", args.join(" ")));
        match &self.fail {
            Some(why) => Err(Fail::Failed(why.clone())),
            None => Ok(()),
        }
    }
    fn admin(&mut self, what: &str, script: &str) -> Result<(), Fail> {
        self.asked.push(format!("admin {what}: {script}"));
        if self.cancel {
            Err(Fail::Cancelled)
        } else {
            Ok(())
        }
    }
    fn terminate(&mut self, pid: u32) -> Result<(), Fail> {
        self.asked.push(format!("term {pid}"));
        Ok(())
    }
    fn kill(&mut self, pid: u32) -> Result<(), Fail> {
        self.asked.push(format!("kill {pid}"));
        Ok(())
    }
    fn alive(&mut self, _: u32) -> bool {
        false
    }
    fn pause(&mut self) {}
    fn remove_dir(&mut self, dir: &Path) -> Result<(), Fail> {
        self.asked.push(format!("rm {}", dir.display()));
        Ok(())
    }
}

/// Repair, step by step (Go `repairSteps`, `repair`).
mod repair_tests {
    use std::sync::Arc;

    use super::Record;
    use crate::settings::machine::{PATH_LINK, Part};
    use crate::settings::repair::{Fail, Outcome, doze_outcome, run, steps};
    use crate::view::StackView;

    fn part(section: &'static str, name: &str, ok: bool) -> Part {
        Part {
            section,
            name: name.into(),
            ok,
            fix: if ok { String::new() } else { "x".into() },
            ..Part::default()
        }
    }

    fn shop() -> StackView {
        let mut st = StackView::new(crate::StackKey::new("/k"));
        st.name = "shop".into();
        st.dir = "/work/shop".into();
        st.instances = Arc::new(vec![doze_control::InstanceView {
            name: "web".into(),
            domains: vec!["shop.test".into()],
            ..Default::default()
        }]);
        st
    }

    #[test]
    fn each_failing_section_is_one_step() {
        let parts = [
            part("names", "network", false),
            part("domains", "route shop.test", false),
            part("app", "doze on PATH", false),
        ];
        let s = steps(
            &[shop()],
            &parts,
            Some(std::path::Path::new("/Users/x/go/bin/doze")),
        );
        let what: Vec<&str> = s.iter().map(|s| s.what.as_str()).collect();
        assert_eq!(
            what,
            ["the .doze names", "shop's domains", "put doze on your PATH"]
        );
        assert_eq!(s[1].args, ["trust"]);
        assert_eq!(
            s[2].script,
            format!("mkdir -p /usr/local/bin\nln -sf '/Users/x/go/bin/doze' {PATH_LINK}")
        );
        // With no stack declaring domains, stale routes are pruned by the app itself.
        let stale = [part("domains", "stale route old.test", false)];
        let s = steps(&[], &stale, None);
        assert!(s.len() == 1 && s[0].prune);
        assert!(steps(&[], &[part("names", "network", true)], None).is_empty());
    }

    #[test]
    fn runs_in_order_and_stops_at_a_cancel_or_a_failure() {
        let parts = [
            part("names", "network", false),
            part("domains", "route shop.test", false),
        ];
        let s = steps(&[shop()], &parts, None);
        let mut hands = Record::default();
        let out = run(&s, &mut hands, &mut |_| Ok(()));
        assert_eq!(out, Outcome::Done);
        assert_eq!(hands.asked, ["doze dns-setup", "doze trust in /work/shop"]);
        let mut failing = Record {
            fail: Some("no network".into()),
            ..Record::default()
        };
        let out = run(&s, &mut failing, &mut |_| Ok(()));
        assert_eq!(
            out.words(&[]).0,
            "setting up the .doze names failed: no network"
        );
        assert_eq!(failing.asked.len(), 1);
        assert_eq!(run(&[], &mut hands, &mut |_| Ok(())), Outcome::Nothing);
        let still = [part("names", "network", false)];
        assert_eq!(
            Outcome::Done.words(&still).0,
            "repaired what could be; still missing: network"
        );
        assert_eq!(Outcome::Done.words(&[]).0, "this machine is set up");
    }

    #[test]
    fn a_doze_commands_output_says_how_it_went() {
        assert_eq!(doze_outcome(true, "anything"), Ok(()));
        assert_eq!(
            doze_outcome(false, "x\ndoze: the administrator prompt was cancelled\n"),
            Err(Fail::Cancelled)
        );
        assert_eq!(
            doze_outcome(false, "setting up\nresolver: permission denied\n"),
            Err(Fail::Failed("resolver: permission denied".into()))
        );
    }
}
