//! The tabs an instance's page opens with, by kind: every kind opens in the same frame and only the front tab
//! differs. The CLI and the app show the same tabs in the same order because both take them from here.

use std::borrow::Cow;

use crate::console::{ConsoleSpec, Node, NodeKind, PanelKind};
use crate::model::{Instance, Kind};

/// One tab of an instance's page: its id, which the keys and the console spec refer to, and the title drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab<'a> {
    /// The id: one of the `TAB_` constants, or `console:<id>` for a tab a module's console adds.
    pub id: Cow<'a, str>,
    /// The title.
    pub title: Cow<'a, str>,
}

impl Tab<'static> {
    const fn fixed(id: &'static str) -> Self {
        Self {
            id: Cow::Borrowed(id),
            title: Cow::Borrowed(id),
        }
    }
}

/// The logs tab.
pub const TAB_LOGS: &str = "logs";
/// The overview tab.
pub const TAB_OVERVIEW: &str = "overview";
/// A task's last run.
pub const TAB_LAST_RUN: &str = "last run";
/// A console of the instance's own, opened in a browser.
pub const TAB_WEB: &str = "web console";

/// It has a console of its own to open in a browser. (Go: `HasWeb`.)
#[must_use]
pub fn has_web(v: &Instance) -> bool {
    !v.console.is_empty() || !v.web_url.is_empty()
}

/// The address of that console: the one the config gave, else the module's. (Go: `WebURL`.)
#[must_use]
pub fn web_url(v: &Instance) -> &str {
    if v.console.is_empty() {
        &v.web_url
    } else {
        &v.console
    }
}

/// The tabs an instance opens with: the one it opens on first, then the rest. `spec` is the console the module
/// described, once it has been received; before then a console still gets one tab called "console". (Go: `Tabs`.)
#[must_use]
pub fn tabs<'a>(v: &Instance, spec: Option<&'a ConsoleSpec>) -> Vec<Tab<'a>> {
    // Only a module's page is customised, and only by what the module declares (its console spec): an app, a task
    // or an outside service is the standard page, whatever it claims (the owner's rule).
    if v.has_console && v.kind == Kind::Module {
        let mut out = Vec::with_capacity(spec.map_or(1, |s| s.tabs.len()) + 2);
        if !v.console_front.is_empty() && v.console_front != "console" {
            out.push(Tab::fixed(TAB_OVERVIEW));
        }
        out.extend(console_tabs(spec));
        out.push(Tab::fixed(TAB_LOGS));
        return out;
    }
    let first = match v.kind {
        // Its last run first, as the Task board draws it; the logs tab beside it filters and searches every run.
        Kind::Task => TAB_LAST_RUN,
        // A module that ships a good UI opens on it; a service told where its own is keeps the standard page.
        Kind::Module if has_web(v) => TAB_WEB,
        Kind::Module | Kind::Unspecified => TAB_OVERVIEW,
        Kind::App | Kind::Service | Kind::Other => return vec![Tab::fixed(TAB_LOGS)],
    };
    vec![Tab::fixed(first), Tab::fixed(TAB_LOGS)]
}

/// The tabs a console adds, from its spec: each of its tabs, or one called "console" for a spec of panels with
/// none. (Go: `ConsoleTabs`.)
#[must_use]
pub fn console_tabs(spec: Option<&ConsoleSpec>) -> Vec<Tab<'_>> {
    let whole = || Tab {
        id: Cow::Borrowed("console:_"),
        title: Cow::Borrowed("console"),
    };
    let Some(spec) = spec else {
        return vec![whole()];
    };
    if spec.tabs.is_empty() {
        return if spec.panels.is_empty() {
            Vec::new()
        } else {
            vec![whole()]
        };
    }
    spec.tabs
        .iter()
        .map(|t| Tab {
            id: Cow::Owned(format!("console:{}", t.id)),
            title: Cow::Borrowed(&t.title),
        })
        .collect()
}

/// The layout of the tab `tab_id`: the one the module published, or for a spec of panels with no tabs, its tables
/// side by side, its streams below, then its actions. `None` for a tab the spec does not have. (Go: `ConsoleBody`.)
#[must_use]
pub fn console_body<'a>(spec: Option<&'a ConsoleSpec>, tab_id: &str) -> Option<Cow<'a, Node>> {
    let spec = spec?;
    let id = tab_id.strip_prefix("console:");
    if let Some(t) = spec.tabs.iter().find(|t| id == Some(t.id.as_str())) {
        return Some(Cow::Borrowed(&t.body));
    }
    if !spec.tabs.is_empty() {
        return None;
    }
    let mut tables = Vec::new();
    let mut kids = Vec::new();
    for p in &spec.panels {
        match p.kind {
            PanelKind::Table => tables.push(Node::block(NodeKind::Table, p)),
            PanelKind::Counters => tables.push(Node::block(NodeKind::Stat, p)),
            PanelKind::Stream => kids.push(Node {
                from: p.select.clone(),
                ..Node::block(NodeKind::Stream, p)
            }),
            PanelKind::Other => {}
        }
    }
    if !tables.is_empty() {
        kids.insert(
            0,
            Node {
                kind: NodeKind::Row,
                weight: 1,
                children: tables,
                ..Node::default()
            },
        );
    }
    kids.push(Node {
        kind: NodeKind::Actions,
        ..Node::default()
    });
    Some(Cow::Owned(Node {
        kind: NodeKind::Col,
        children: kids,
        ..Node::default()
    }))
}

#[cfg(test)]
mod owner_rule_tests {
    use super::{TAB_LAST_RUN, TAB_LOGS, TAB_OVERVIEW, TAB_WEB, tabs};
    use crate::model::{Instance, Kind};

    fn ids(v: &Instance) -> Vec<String> {
        tabs(v, None)
            .into_iter()
            .map(|t| t.id.into_owned())
            .collect()
    }

    /// Only a module's page is customised: an app, a task, an outside service or an instance of no kind that carries
    /// a console or a web address still gets the standard page's tabs (the owner's rule).
    #[test]
    fn only_a_module_gets_console_or_web_tabs() {
        let custom = |kind| Instance {
            kind,
            has_console: true,
            console_front: "console".into(),
            web_url: "http://ui.doze".into(),
            ..Instance::default()
        };
        for kind in [Kind::App, Kind::Service, Kind::Other] {
            assert_eq!(ids(&custom(kind)), [TAB_LOGS], "{kind:?}");
        }
        assert_eq!(ids(&custom(Kind::Task)), [TAB_LAST_RUN, TAB_LOGS]);
        assert_eq!(ids(&custom(Kind::Unspecified)), [TAB_OVERVIEW, TAB_LOGS]);
        let module = ids(&custom(Kind::Module));
        assert!(
            module.iter().any(|t| t.starts_with("console:")),
            "{module:?}"
        );
        let web = Instance {
            has_console: false,
            ..custom(Kind::Module)
        };
        assert_eq!(ids(&web), [TAB_WEB, TAB_LOGS]);
    }
}
