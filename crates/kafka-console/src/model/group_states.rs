//! The states of the Groups list that are on, as a set.

use super::groups::{GroupFilter, GroupPhase};

/// The states of the list that are on: a group shows when it is in ANY of them; none on means every group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupStates(Vec<GroupFilter>);

impl GroupStates {
    /// From every `state=` of an address (repeated: `state=stable&state=empty`); a word that is no state is left out.
    #[must_use]
    pub fn parse(fields: &[(String, String)]) -> Self {
        let mut v: Vec<GroupFilter> = Vec::new();
        for (k, val) in fields {
            let s = GroupFilter::parse(val.trim());
            if k == "state" && s != GroupFilter::All && !v.contains(&s) {
                v.push(s);
            }
        }
        v.sort_by_key(|s| *s as u8);
        Self(v)
    }

    /// No state chosen (every group shows).
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether this state is on.
    #[must_use]
    pub fn has(&self, s: GroupFilter) -> bool {
        self.0.contains(&s)
    }

    /// The values to put in an address, one per state on.
    #[must_use]
    pub fn values(&self) -> Vec<&'static str> {
        self.0.iter().map(|s| s.value()).collect()
    }

    /// Whether a group in `phase` shows.
    #[must_use]
    pub fn takes(&self, phase: GroupPhase) -> bool {
        self.0.is_empty() || self.0.iter().any(|s| s.takes(phase))
    }

    /// The address's query part for a name filter and these states.
    #[must_use]
    pub fn query(&self, filter: &str) -> String {
        let mut fields: Vec<(&str, &str)> = vec![("q", filter)];
        fields.extend(self.values().into_iter().map(|v| ("state", v)));
        crate::form::encode_query(&fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &str) -> GroupStates {
        GroupStates::parse(&crate::form::parse(q))
    }

    #[test]
    fn states_are_a_repeated_field_and_a_group_shows_in_any_of_them() {
        let w = s("state=empty&state=stable&state=stable&state=nonsense&state=");
        assert_eq!(w.values(), ["stable", "empty"]);
        assert!(w.has(GroupFilter::Stable) && !w.has(GroupFilter::Rebalancing));
        assert!(
            w.takes(GroupPhase::Stable)
                && w.takes(GroupPhase::Empty)
                && !w.takes(GroupPhase::Rebalancing)
        );
        assert!(s("").is_all() && s("").takes(GroupPhase::Rebalancing));
        assert_eq!(w.query("pay"), "?q=pay&state=stable&state=empty");
        assert_eq!(s("state=all").values(), Vec::<&str>::new());
    }
}
