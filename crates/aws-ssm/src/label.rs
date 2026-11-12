//! Labels: names for versions. A label lives on one version of a parameter at a time, and a version holds at most ten.

use aws_model_ssm::keys::{
    LabelParameterVersionRequest as Label, LabelParameterVersionResult as Labelled,
    UnlabelParameterVersionRequest as Unlabel, UnlabelParameterVersionResult as Unlabelled,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, reply, request};
use crate::read::version_not_found;
use crate::write::not_found;

const MAX_LABELS: usize = 10;

/// Whether a label is acceptable: 1 to 100 of letters, digits, `.`, `-`, `_`; not starting with a digit, `aws` or `ssm`.
fn valid(label: &str) -> bool {
    let lower = label.to_ascii_lowercase();
    (1..=100).contains(&label.len())
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !label.starts_with(|c: char| c.is_ascii_digit())
        && !lower.starts_with("aws")
        && !lower.starts_with("ssm")
}

impl Core {
    pub(crate) fn label_parameter_version(&mut self, call: &Call) -> Reply {
        let v = request::<Label>(call)?;
        let name = v.get(Label::NAME).unwrap_or_default();
        let labels: Vec<&str> = v
            .get(Label::LABELS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let p = self.params.get_mut(name).ok_or_else(not_found)?;
        let number = v.get(Label::PARAMETER_VERSION).unwrap_or(p.latest.number);
        let (good, bad): (Vec<&str>, Vec<&str>) = labels.into_iter().partition(|l| valid(l));
        let held = p
            .version(number)
            .ok_or_else(|| version_not_found(name, &number.to_string()))?;
        let fresh = good.iter().filter(|l| !held.labels.contains(**l)).count();
        if held.labels.len() + fresh > MAX_LABELS {
            return Err(ApiError::modeled(
                "ParameterVersionLabelLimitExceeded",
                "A parameter version can have maximum 10 labels. Move one or more labels to another version and try again.",
            ));
        }
        for label in &good {
            // A label is on one version: moving it takes it off the version that had it.
            for old in p.older.iter_mut().chain(std::iter::once(&mut p.latest)) {
                old.labels.remove(*label);
            }
        }
        if let Some(target) = p.version_mut(number) {
            target.labels.extend(good.iter().map(|l| (*l).to_owned()));
        }
        reply::<Labelled>(|o| {
            o.put_list(Labelled::INVALID_LABELS, |l| {
                for b in &bad {
                    l.push(b);
                }
            });
            o.put_int(Labelled::PARAMETER_VERSION, number);
        })
    }

    pub(crate) fn unlabel_parameter_version(&mut self, call: &Call) -> Reply {
        let v = request::<Unlabel>(call)?;
        let name = v.get(Unlabel::NAME).unwrap_or_default();
        let number = v.get(Unlabel::PARAMETER_VERSION).unwrap_or_default();
        let labels: Vec<&str> = v
            .get(Unlabel::LABELS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let p = self.params.get_mut(name).ok_or_else(not_found)?;
        let target = p
            .version_mut(number)
            .ok_or_else(|| version_not_found(name, &number.to_string()))?;
        let (removed, absent): (Vec<&str>, Vec<&str>) =
            labels.into_iter().partition(|l| target.labels.remove(*l));
        reply::<Unlabelled>(|o| {
            o.put_list(Unlabelled::REMOVED_LABELS, |l| {
                for r in &removed {
                    l.push(r);
                }
            });
            o.put_list(Unlabelled::INVALID_LABELS, |l| {
                for a in &absent {
                    l.push(a);
                }
            });
        })
    }
}

#[cfg(test)]
mod tests {
    use super::valid;

    #[test]
    fn a_label_follows_the_rules() {
        assert!(valid("stable"));
        assert!(valid("v1.2-final_x"));
        assert!(!valid("1st"));
        assert!(!valid("aws-owned"));
        assert!(!valid("SSM"));
        assert!(!valid(""));
        assert!(!valid("has space"));
    }
}
