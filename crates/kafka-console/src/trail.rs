//! The breadcrumb, in one place and by one rule: it is the ENTITY PATH and nothing else.
//!
//! `Resources > Topics > orders > Partitions`, `Resources > Groups > fulfilment > Members`, `Live`, `Settings`. It is never
//! a filter, a mode, a partition chip, an isolation level, a tab state or a state of the page: those are state of the entity
//! the crumb names, and the page shows them where they are chosen. The rule is kept by the types: [`Place`] has a field for
//! every entity and none for a query, so a handler that has a filter in its hand cannot put it in the trail.
//!
//! The `<title>` of a page is its last crumb (the shell does that, `Shared::document_over`), so the tab says what the
//! breadcrumb says.

use crate::base::segment;

/// One step: its label and where it goes (a path under the base); the last has no link.
pub type Step = (String, Option<String>);

/// The longest entity name a crumb shows, in characters: a topic name may be 249.
const SHOWN: usize = 60;

/// The pages of a topic that are a step under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicPage {
    Partitions,
    Config,
    Log,
    Consumers,
    Producers,
    Wire,
}

impl TopicPage {
    const fn label(self) -> &'static str {
        match self {
            Self::Partitions => "Partitions",
            Self::Config => "Config",
            Self::Log => "Log",
            Self::Consumers => "Consumers",
            Self::Producers => "Producers",
            Self::Wire => "On the wire",
        }
    }
}

/// A place in the console: where the page is, never how it is filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place<'a> {
    Home,
    Connect,
    Advertised,
    FirstRequest,
    Topics,
    NewTopic,
    /// A topic, on any of its Messages states (a partition, an isolation level, a time are not part of the place).
    Topic(&'a str),
    TopicPage(&'a str, TopicPage),
    /// A name that is no topic.
    NoTopic,
    Groups,
    Group(&'a str),
    /// A page under a group (`Members`, `Lag`, a member's id).
    GroupPage(&'a str, &'a str),
    NoGroup,
    Live,
    Access,
    Settings,
    Lab,
    NotFound,
    NotAllowed,
}

fn shown(name: &str) -> String {
    if name.chars().count() <= SHOWN {
        return name.to_owned();
    }
    let mut cut: String = name.chars().take(SHOWN - 1).collect();
    cut.push('…');
    cut
}

fn link(label: &str, path: &str) -> Step {
    (label.to_owned(), Some(path.to_owned()))
}

fn last(label: &str) -> Step {
    (label.to_owned(), None)
}

fn resources() -> [Step; 1] {
    [link("Resources", "/topics")]
}

fn topic_path(name: &str) -> String {
    format!("/topics/{}", segment(name))
}

fn group_path(name: &str) -> String {
    format!("/groups/{}", segment(name))
}

impl Place<'_> {
    /// The steps of the breadcrumb, paths under the base.
    #[must_use]
    pub fn steps(self) -> Vec<Step> {
        match self {
            Self::Home => vec![last("Home")],
            Self::Connect => vec![link("Home", "/"), last("Connect an app")],
            Self::Advertised => vec![
                link("Home", "/"),
                link("Connect an app", "/connect"),
                last("Advertised listener"),
            ],
            Self::FirstRequest => vec![
                link("Home", "/"),
                link("Connect an app", "/connect"),
                last("First request landed"),
            ],
            Self::Topics => with(&resources(), vec![last("Topics")]),
            Self::NewTopic => with(
                &resources(),
                vec![link("Topics", "/topics"), last("New topic")],
            ),
            Self::Topic(name) => with(
                &resources(),
                vec![link("Topics", "/topics"), last(&shown(name))],
            ),
            Self::TopicPage(name, page) => with(
                &resources(),
                vec![
                    link("Topics", "/topics"),
                    link(&shown(name), &topic_path(name)),
                    last(page.label()),
                ],
            ),
            Self::NoTopic => with(
                &resources(),
                vec![link("Topics", "/topics"), last("No such topic")],
            ),
            Self::Groups => with(&resources(), vec![last("Groups")]),
            Self::Group(name) => with(
                &resources(),
                vec![link("Groups", "/groups"), last(&shown(name))],
            ),
            Self::GroupPage(name, page) => with(
                &resources(),
                vec![
                    link("Groups", "/groups"),
                    link(&shown(name), &group_path(name)),
                    last(&shown(page)),
                ],
            ),
            Self::NoGroup => with(
                &resources(),
                vec![link("Groups", "/groups"), last("No such group")],
            ),
            Self::Live => vec![last("Live")],
            Self::Access => vec![last("Access")],
            Self::Settings => vec![last("Settings")],
            Self::Lab => vec![last("Lab")],
            Self::NotFound => vec![link("Home", "/"), last("Not found")],
            Self::NotAllowed => vec![link("Home", "/"), last("Not allowed")],
        }
    }
}

fn with(head: &[Step], tail: Vec<Step>) -> Vec<Step> {
    head.iter().cloned().chain(tail).collect()
}

/// The steps as the borrowed pairs `Shared::document` takes.
#[must_use]
pub fn borrowed(steps: &[Step]) -> Vec<(&str, Option<&str>)> {
    steps
        .iter()
        .map(|(label, path)| (label.as_str(), path.as_deref()))
        .collect()
}

#[cfg(test)]
mod tests;
