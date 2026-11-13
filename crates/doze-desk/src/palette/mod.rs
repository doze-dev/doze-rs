//! ⌘K, the command palette (Go `view.PaletteOf`, `view/palette.go` and `view/commands.go`). With nothing typed: what
//! needs you, a few commands for the page on screen, what you did lately, and every stack to tab into. A name typed
//! finds services and stacks across every stack; a name then words is a command for it ("api restart") or its logs
//! searched ("api timeout"); a verb then a name finds what the verb applies to, with "Did you mean" under it; nothing
//! found says so and offers the closest name. Tab on a stack or a service narrows the palette to its commands.
//!
//! The model is plain data, written out as Go's JSON writes it, so it is held to Go's goldens field for field.

mod commands;
mod find;
mod instance;
mod recent;
mod stack;
mod verbs;

use std::time::SystemTime;

use serde::{Serialize, Serializer};

use crate::view::StackView;

pub use commands::filter_rows;
pub use instance::{client_line, instance_commands};
pub use recent::{RECENT_KEPT, RecentCommand, RecentService, opened, remember};
pub use stack::{app_commands, stack_commands};
pub use verbs::{ALIASES, VERBS, edit_distance};

/// The palette for one query.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Palette {
    pub query: String,
    /// Where the palette is narrowed to, by name: `["shop"]` inside a stack, `["shop", "api"]` inside a service.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub crumbs: Vec<String>,
    pub sections: Vec<Section>,
    /// Set when the query matched nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nothing: Option<Nothing>,
}

impl Palette {
    /// Every row, in the order shown (the highlight counts these).
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }
}

/// One heading of the list.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Row>,
}

/// One line: a glyph, the name in bold with the matched characters marked, the words after it, the key at the right,
/// the command line that does the same, and what ⏎ does.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Row {
    /// `dot` (with [`Row::tone`] the dot's colour) or a character.
    pub glyph: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub tone: &'static str,
    pub label: String,
    /// The matched characters of the label (char indices).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub at: Vec<usize>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The same from a terminal: `doze sleep api`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cli: String,
    /// Destructive: drawn in the warning colour.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bad: bool,
    #[serde(rename = "do")]
    pub doing: Doing,
    /// The row's own actions, the most likely first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub acts: Vec<Act>,
    /// What tab narrows the palette to; `None` for a command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enter: Option<Scope>,
    /// The words a row is found by besides its label.
    #[serde(skip)]
    pub also: &'static str,
}

/// One of a row's actions: the word on the chip and what it does.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Act {
    pub label: &'static str,
    #[serde(rename = "do")]
    pub doing: Doing,
}

/// The no-match line, and the closest name.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Nothing {
    pub name: String,
    /// Where it looked: "shop or acme".
    pub r#where: String,
    pub closest: String,
    /// "kafka, in shop".
    pub why: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(rename = "do", skip_serializing_if = "Option::is_none")]
    pub doing: Option<Doing>,
    /// The corrected command a verb with a near name becomes: ⏎ puts it in the box to be read and run, never runs the
    /// verb on a guess.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fill: String,
}

/// What choosing a row does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Doing {
    pub kind: DoKind,
    /// The stack it is about (its key).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stack_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub verb: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// The verb asks first (a dialog).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// The kinds of doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DoKind {
    /// Runs `verb` on `target` (or on the whole stack), through its dialog when `ask`.
    #[default]
    Verb,
    /// Goes to `page` (`home`, `instance`, `logs`, `activity`) of `target`; `text` filters logs.
    Page,
    /// Copies `text`.
    Copy,
    /// Opens `text` (a URL or a `file://` path).
    Open,
    /// A stack action: `verb` is `up-last`, `up-all`, `restart`, `down` or `remove`.
    Stack,
    /// Asks for a folder.
    Folder,
    /// Opens the start sheet.
    Choose,
    /// Opens a terminal in `text`.
    Terminal,
    /// One of the app's own commands, by id in `text` (`home`, `bell`, `theme:teal`, …).
    App,
}

/// Where tab narrowed the palette: a stack, or a service in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    pub key: String,
    pub name: Option<String>,
}

impl Scope {
    /// The scope one step out: a service's stack, or the top (`None`).
    #[must_use]
    pub fn out(&self) -> Option<Scope> {
        self.name.as_ref().map(|_| Scope {
            key: self.key.clone(),
            name: None,
        })
    }
}

impl Serialize for Scope {
    /// Go's form: `key`, or `key#name` (a key is a folder, so never `/`).
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.name {
            Some(n) => s.serialize_str(&format!("{}#{n}", self.key)),
            None => s.serialize_str(&self.key),
        }
    }
}

/// What the palette is asked: where the person is, and what they typed.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// The stack on screen (its key), whose commands come first.
    pub current: &'a str,
    /// The service on screen.
    pub inst: &'a str,
    /// What tab narrowed to.
    pub scope: Option<&'a Scope>,
    /// The services opened lately, newest first.
    pub recent: &'a [RecentService],
    /// The commands done lately, newest first.
    pub cmds: &'a [RecentCommand],
    pub query: &'a str,
    pub now: SystemTime,
}

/// A stack's key as the palette writes it.
pub(crate) fn key_of(st: &StackView) -> String {
    st.key.dir().to_string_lossy().into_owned()
}

/// The palette for `ask` across `stacks` (Go `PaletteOf`).
#[must_use]
pub fn palette(stacks: &[StackView], ask: &Ask<'_>) -> Palette {
    let query = ask.query.trim();
    let mut p = Palette {
        query: query.to_owned(),
        ..Palette::default()
    };
    if let Some(scope) = ask.scope
        && let Some(st) = stacks.iter().find(|s| key_of(s) == scope.key)
    {
        let (sections, crumbs) = commands::scoped(st, scope.name.as_deref(), query, ask.now);
        p.sections = sections;
        if p.sections.is_empty() && !query.is_empty() {
            p.nothing = Some(Nothing {
                name: query.to_owned(),
                r#where: crumbs.join(" › "),
                ..Nothing::default()
            });
        }
        p.crumbs = crumbs;
        return p;
    }
    // The current stack first, then the rest as the Library orders them.
    let mut ordered: Vec<&StackView> = stacks.iter().collect();
    ordered.sort_by_key(|s| key_of(s) != ask.current);
    if query.is_empty() {
        p.sections = find::at_hand(&ordered, ask);
        return p;
    }
    let lower = query.to_lowercase();
    let (first, rest) = lower.split_once(' ').unwrap_or((lower.as_str(), ""));
    let verb = verbs::alias(first);
    if let Some(s) = find::by_object(&ordered, ask.current, verb, rest.trim(), ask.now) {
        p.sections = s;
        return p;
    }
    if VERBS.contains(&verb) {
        p.sections = verbs::for_verb(&ordered, verb, rest.trim(), ask.now);
        if p.sections.is_empty() {
            p.nothing = Some(verbs::nothing(&ordered, verb, rest.trim(), ask.now));
        }
        return p;
    }
    p.sections = find::by_name(&ordered, query, ask.now);
    let cmds = filter_rows(find::here(&ordered, ask), query);
    if !cmds.is_empty() {
        p.sections.push(Section {
            title: "Commands".into(),
            rows: cmds.into_iter().take(8).collect(),
        });
    }
    if p.sections.is_empty() {
        p.nothing = Some(verbs::nothing(&ordered, "", query, ask.now));
    }
    p
}

/// A service as the palette reads it: the words' instance, and the two wire fields the words leave out.
#[derive(Debug, Clone, Copy)]
pub struct Svc<'a> {
    pub(crate) v: &'a doze_display::Instance,
    pub(crate) https: &'a str,
    pub(crate) env_var: &'a str,
}

impl<'a> Svc<'a> {
    /// `v` of `st`, with its wire twin's fields.
    pub(crate) fn of(st: &'a StackView, v: &'a doze_display::Instance) -> Svc<'a> {
        let w = st.instance(&v.name);
        Svc {
            v,
            https: w.map_or("", |w| w.https.as_str()),
            env_var: w.map_or("", |w| w.env_var.as_str()),
        }
    }
}

/// The dot's colour for a look (Go `dotOf`).
pub(crate) fn dot_of(look: doze_display::Look) -> &'static str {
    use doze_display::Tone;
    match look.tone() {
        Tone::Good => "g",
        Tone::Warn => "y",
        Tone::Bad => "r",
        Tone::Cool => "b",
        _ => "",
    }
}

/// The first of `ss` that is not empty.
pub(crate) fn first_of<'a>(ss: &[&'a str]) -> &'a str {
    ss.iter().copied().find(|s| !s.is_empty()).unwrap_or("")
}

/// Where a module or an app serves a web page to open (Go `webURL`).
pub(crate) fn web_url(v: &doze_display::Instance) -> &str {
    if !v.console.is_empty() {
        return &v.console;
    }
    if v.kind == doze_display::Kind::App && v.url.starts_with("http") {
        return &v.url;
    }
    ""
}

/// Whether `verb` on `v` asks first (Go: `DialogOf(…).Needed` for sleep, restart and off): only when something else
/// is affected.
pub(crate) fn asks(v: &doze_display::Instance, verb: &str) -> bool {
    let used = doze_display::in_use(v).is_some();
    match verb {
        "sleep" | "off" => used,
        "restart" => used && doze_display::awake_now(v) && v.kind != doze_display::Kind::Task,
        "reset" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
