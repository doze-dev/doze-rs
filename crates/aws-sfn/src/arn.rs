//! Step Functions ARNs: `arn:aws:states:<region>:<account>:<kind>:<rest>`. A state machine is `stateMachine:<name>`, a version
//! `stateMachine:<name>:<number>`, an alias `stateMachine:<name>:<alias>`, an execution `execution:<machine>:<name>`, an Express
//! execution `express:<machine>:<name>:<id>`, an activity `activity:<name>`, a Map Run `mapRun:<machine>/<label>:<id>`.

/// The parts of a Step Functions ARN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parts<'a> {
    pub(crate) region: &'a str,
    pub(crate) account: &'a str,
    pub(crate) kind: &'a str,
    /// What follows the kind, split on `:`.
    pub(crate) rest: Vec<&'a str>,
}

pub(crate) fn parts(arn: &str) -> Option<Parts<'_>> {
    let mut it = arn.splitn(7, ':');
    let (a, p, s) = (it.next()?, it.next()?, it.next()?);
    if a != "arn" || !p.starts_with("aws") || s != "states" {
        return None;
    }
    let (region, account, kind) = (it.next()?, it.next()?, it.next()?);
    let rest: Vec<&str> = it
        .next()
        .map(|r| r.split(':').collect())
        .unwrap_or_default();
    if region.is_empty() || account.is_empty() || rest.iter().any(|r| r.is_empty()) {
        return None;
    }
    Some(Parts {
        region,
        account,
        kind,
        rest,
    })
}

/// What a `stateMachineArn` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Machine(String),
    Version(String, u64),
    Alias(String, String),
}

impl Target {
    pub(crate) fn machine(&self) -> &str {
        match self {
            Self::Machine(m) | Self::Version(m, _) | Self::Alias(m, _) => m,
        }
    }
}

/// Reads a state machine, version or alias ARN; `None` when it is not one.
pub(crate) fn target(arn: &str) -> Option<(Parts<'_>, Target)> {
    let p = parts(arn)?;
    if p.kind != "stateMachine" {
        return None;
    }
    let t = match p.rest.as_slice() {
        [m] => Target::Machine((*m).to_owned()),
        [m, q] => match q.parse::<u64>() {
            Ok(n) => Target::Version((*m).to_owned(), n),
            Err(_) => Target::Alias((*m).to_owned(), (*q).to_owned()),
        },
        _ => return None,
    };
    Some((p, t))
}

pub(crate) fn machine(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:states:{region}:{account}:stateMachine:{name}")
}

pub(crate) fn execution(region: &str, account: &str, machine: &str, name: &str) -> String {
    format!("arn:aws:states:{region}:{account}:execution:{machine}:{name}")
}

pub(crate) fn express(region: &str, account: &str, machine: &str, name: &str, id: &str) -> String {
    format!("arn:aws:states:{region}:{account}:express:{machine}:{name}:{id}")
}

pub(crate) fn activity(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:states:{region}:{account}:activity:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arns() {
        let (p, t) = target("arn:aws:states:us-east-1:1:stateMachine:m:3").unwrap();
        assert_eq!((p.region, t), ("us-east-1", Target::Version("m".into(), 3)));
        assert_eq!(
            target("arn:aws:states:us-east-1:1:stateMachine:m:prod")
                .unwrap()
                .1,
            Target::Alias("m".into(), "prod".into())
        );
        assert!(target("arn:aws:states:us-east-1:1:activity:a").is_none());
        assert!(target("not-an-arn").is_none());
    }
}
