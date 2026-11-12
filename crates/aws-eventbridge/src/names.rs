//! The names and ARNs of EventBridge's things. A rule on the default bus is `rule/<name>`; on any other bus, `rule/<bus>/<name>`
//! (the second is recorded 2026-11-09, the first is how AWS has always written it, from memory).

/// The bus every account has.
pub(crate) const DEFAULT_BUS: &str = "default";

pub(crate) fn bus_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:events:{region}:{account}:event-bus/{name}")
}

pub(crate) fn rule_arn(region: &str, account: &str, bus: &str, name: &str) -> String {
    if bus == DEFAULT_BUS {
        format!("arn:aws:events:{region}:{account}:rule/{name}")
    } else {
        format!("arn:aws:events:{region}:{account}:rule/{bus}/{name}")
    }
}

pub(crate) fn archive_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:events:{region}:{account}:archive/{name}")
}

pub(crate) fn replay_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:events:{region}:{account}:replay/{name}")
}

/// A connection or an API destination: AWS adds a UUID to the name (`connection/<name>/<uuid>`).
pub(crate) fn with_uuid(region: &str, account: &str, kind: &str, name: &str, uuid: &str) -> String {
    format!("arn:aws:events:{region}:{account}:{kind}/{name}/{uuid}")
}

/// The name of a bus given its name or its ARN. `None` for an ARN that is not a bus's.
pub(crate) fn bus_name(name_or_arn: &str) -> Option<&str> {
    if name_or_arn.starts_with("arn:") {
        Parts::of(name_or_arn)
            .filter(|p| p.kind == "event-bus")
            .map(|p| p.tail)
    } else {
        Some(name_or_arn)
    }
}

/// An EventBridge ARN taken apart: `arn:aws:events:<region>:<account>:<kind>/<tail>`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Parts<'a> {
    pub(crate) region: &'a str,
    pub(crate) account: &'a str,
    pub(crate) kind: &'a str,
    /// Everything after the first slash.
    pub(crate) tail: &'a str,
}

impl<'a> Parts<'a> {
    pub(crate) fn of(arn: &'a str) -> Option<Self> {
        let mut it = arn.splitn(6, ':');
        let (Some("arn"), Some(_), Some("events"), Some(region), Some(account), Some(rest)) = (
            it.next(),
            it.next(),
            it.next(),
            it.next(),
            it.next(),
            it.next(),
        ) else {
            return None;
        };
        let (kind, tail) = rest.split_once('/')?;
        Some(Self {
            region,
            account,
            kind,
            tail,
        })
    }

    /// For a rule: the bus and the rule's name.
    pub(crate) fn rule(self) -> Option<(&'a str, &'a str)> {
        if self.kind != "rule" {
            return None;
        }
        Some(match self.tail.split_once('/') {
            Some((bus, name)) => (bus, name),
            None => (DEFAULT_BUS, self.tail),
        })
    }
}

/// The service of any ARN (`sqs` in `arn:aws:sqs:...`).
pub(crate) fn service_of(arn: &str) -> &str {
    arn.split(':').nth(2).unwrap_or_default()
}

/// The region and account of any ARN.
pub(crate) fn region_and_account(arn: &str) -> (&str, &str) {
    let mut it = arn.split(':').skip(3);
    (it.next().unwrap_or_default(), it.next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arns_round_trip_and_the_default_bus_leaves_its_name_out_of_a_rule() {
        let custom = rule_arn("us-east-1", "111122223333", "shop", "r");
        assert_eq!(custom, "arn:aws:events:us-east-1:111122223333:rule/shop/r");
        assert_eq!(
            Parts::of(&custom).and_then(Parts::rule),
            Some(("shop", "r"))
        );
        let default = rule_arn("us-east-1", "111122223333", DEFAULT_BUS, "r");
        assert_eq!(default, "arn:aws:events:us-east-1:111122223333:rule/r");
        assert_eq!(
            Parts::of(&default).and_then(Parts::rule),
            Some(("default", "r"))
        );
        assert_eq!(bus_name(&bus_arn("us-east-1", "1", "shop")), Some("shop"));
        assert_eq!(bus_name("shop"), Some("shop"));
        assert_eq!(bus_name(&custom), None);
        assert_eq!(service_of("arn:aws:sqs:us-east-1:1:q"), "sqs");
        assert_eq!(
            region_and_account("arn:aws:sqs:eu-west-1:42:q"),
            ("eu-west-1", "42")
        );
    }
}
