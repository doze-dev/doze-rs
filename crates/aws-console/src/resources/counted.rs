//! Which services the Resources index counts.

/// Which services the index counts: a count is a list call, and a list call wakes a service that is asleep.
#[derive(Debug, Clone, Copy)]
pub enum Counted<'a> {
    /// None: the services and their kinds alone, no call made.
    No,
    /// Every served service, asleep or not (waking the sleeping ones).
    All,
    /// The served services in the list (the ones that are awake); a served service that is not in it is left asleep, uncounted.
    Awake(&'a [String]),
}

impl Counted<'_> {
    pub(super) fn counts(self, service: &str) -> bool {
        match self {
            Counted::No => false,
            Counted::All => true,
            Counted::Awake(awake) => awake.iter().any(|a| a == service),
        }
    }
}
