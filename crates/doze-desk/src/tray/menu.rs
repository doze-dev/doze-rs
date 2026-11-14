//! The whole menu as data (Go `app/tray.go`'s `menu`, rethought 2026-11-06): what needs you, at the top and only
//! when something is red or worth doing; one line per stack, each a submenu of what fits its state; then Open doze,
//! Search or do…, Settings…, Check for Updates… and Quit. Beyond Go's menu: a running stack's services, each a
//! submenu with its own wake or sleep, its page and its logs, and the stack's All logs.

use doze_control::Verb;
use doze_display::{self as words, Kind, Look};

use super::{Line, Model, Needs, needs_of, split};
use crate::StackKey;
use crate::attention::{Attention, Fix, Item};
use crate::view::StackView;

/// How many services a stack's Services submenu lists before the rest are a line that opens the stack.
pub const SERVICES_SHOWN: usize = 24;

/// What a line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// Open doze ⌘⇧D: the window, as it was.
    OpenDoze,
    /// Search or do… ⌘K: the window with ⌘K open.
    Palette,
    /// Settings… ⌘,.
    Settings,
    /// Check for Updates….
    Updates,
    /// Quit doze; the stacks keep running.
    Quit,
    /// The window, on a stack.
    OpenStack(StackKey),
    /// A stack's web address, in the browser.
    OpenUrl(String),
    /// A stack's action.
    Stack(StackKey, StackCmd),
    /// A verb on one service, through acting (it asks where the window asks).
    Service(StackKey, String, Verb),
    /// A service's page.
    Instance(StackKey, String),
    /// A service's logs, or the stack's All logs (`None`).
    Logs(StackKey, Option<String>),
    /// A Needs you item's fix, pressed from the menu.
    Fix(Box<(Item, Fix)>),
    /// The page that explains a Needs you item.
    Show(Box<Item>),
    /// Needs you, in the window.
    Bell,
}

/// A stack's action from its submenu (Go: Sleep everything, Restart, Down; Up as last time, Up everything).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackCmd {
    SleepAll,
    Restart,
    Down,
    UpLast,
    UpAll,
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A command, with its key equivalent (`"D"` is ⌘⇧D, `","` is ⌘,; empty for none).
    Item {
        cmd: Cmd,
        title: String,
        key: String,
    },
    /// A line with no command, dimmed.
    Note(String),
    /// A dimmed line whose words change by the second (a countdown): left out of the [`signature`], so it does not
    /// rebuild the menu every second, maybe while it is open (Go's `attnSig` leaves the detail out the same way).
    Detail(String),
    /// A section header ("Stacks", "Needs you · 2").
    Header(String),
    Separator,
    Submenu {
        title: String,
        entries: Vec<Entry>,
    },
}

fn item(cmd: Cmd, title: impl Into<String>) -> Entry {
    Entry::Item {
        cmd,
        title: title.into(),
        key: String::new(),
    }
}

fn keyed(cmd: Cmd, title: &str, key: &str) -> Entry {
    Entry::Item {
        cmd,
        title: title.to_owned(),
        key: key.to_owned(),
    }
}

/// The menu for the model, the bell's list and the stacks it was built from.
#[must_use]
pub fn menu(m: &Model, a: &Attention, stacks: &[StackView], names: &Urls<'_>) -> Vec<Entry> {
    let mut out = needs_entries(&needs_of(a));
    if m.lines.is_empty() {
        out.push(Entry::Note("No stacks yet · open a folder in doze".into()));
    } else {
        out.push(Entry::Header("Stacks".into()));
    }
    let find = |k: &StackKey| stacks.iter().find(|s| &s.key == k);
    let (shown, more) = split(&m.lines);
    out.extend(shown.iter().map(|l| stack_entry(l, find(&l.key), names)));
    if !more.is_empty() {
        // The rest, a submenu away: it scrolls on its own if it must.
        out.push(Entry::Submenu {
            title: format!("More stacks · {}", more.len()),
            entries: more
                .iter()
                .map(|l| stack_entry(l, find(&l.key), names))
                .collect(),
        });
    }
    out.push(Entry::Separator);
    out.push(keyed(Cmd::OpenDoze, "Open doze", "D"));
    out.push(keyed(Cmd::Palette, "Search or do…", "k"));
    out.push(keyed(Cmd::Settings, "Settings…", ","));
    out.push(item(Cmd::Updates, "Check for Updates…"));
    // Quit follows at once, as in Go's menu: no line between.
    out.push(keyed(Cmd::Quit, "Quit doze   ·   stacks keep running", "q"));
    out
}

/// Needs you above the stacks: each red item a submenu with its detail, its fix and Show; the yellow line opens the
/// bell.
fn needs_entries(n: &Needs) -> Vec<Entry> {
    let mut out = Vec::new();
    if !n.header.is_empty() {
        out.push(Entry::Header(n.header.clone()));
    }
    for l in &n.lines {
        let mut sub = Vec::new();
        if !l.detail.is_empty() {
            sub.push(Entry::Detail(l.detail.clone()));
            sub.push(Entry::Separator);
        }
        if let Some(f) = &l.fix {
            sub.push(item(
                Cmd::Fix(Box::new((l.item.clone(), f.clone()))),
                f.label.clone(),
            ));
        }
        sub.push(item(Cmd::Show(Box::new(l.item.clone())), "Show in doze"));
        out.push(Entry::Submenu {
            title: l.title.clone(),
            entries: sub,
        });
    }
    if !n.worth.is_empty() {
        out.push(item(Cmd::Bell, n.worth.clone()));
    }
    if !n.is_empty() {
        out.push(Entry::Separator);
    }
    out
}

/// One stack's line and its submenu of what fits its state.
fn stack_entry(l: &Line, st: Option<&StackView>, names: &Urls<'_>) -> Entry {
    let mark = if l.trouble {
        "⚠︎  "
    } else if l.running {
        "●  "
    } else {
        "○  "
    };
    let mut title = format!("{mark}{}", l.name);
    if !l.sub.is_empty() {
        title = format!("{title}   ·   {}", l.sub);
    }
    let k = &l.key;
    let mut sub = vec![item(Cmd::OpenStack(k.clone()), "Open in doze")];
    if !l.running {
        sub.push(Entry::Separator);
        sub.push(item(
            Cmd::Stack(k.clone(), StackCmd::UpLast),
            "Up as last time",
        ));
        sub.push(item(
            Cmd::Stack(k.clone(), StackCmd::UpAll),
            "Up everything",
        ));
        return Entry::Submenu {
            title,
            entries: sub,
        };
    }
    if let Some(url) = st.and_then(|s| web_url(s, names)) {
        sub.push(item(Cmd::OpenUrl(url.clone()), format!("Open {url}")));
    }
    if let Some(st) = st.filter(|s| !s.instances.is_empty()) {
        sub.push(services(st));
        sub.push(item(Cmd::Logs(k.clone(), None), "All logs"));
    }
    sub.push(Entry::Separator);
    sub.push(item(
        Cmd::Stack(k.clone(), StackCmd::SleepAll),
        "Sleep everything",
    ));
    sub.push(item(Cmd::Stack(k.clone(), StackCmd::Restart), "Restart"));
    sub.push(item(Cmd::Stack(k.clone(), StackCmd::Down), "Down"));
    Entry::Submenu {
        title,
        entries: sub,
    }
}

/// "Services · 5": each one a submenu with what fits its look, its page and its logs.
fn services(st: &StackView) -> Entry {
    let snap = st.words();
    let n = snap.instances.len();
    let mut entries: Vec<Entry> = snap
        .instances
        .iter()
        .take(SERVICES_SHOWN)
        .map(|v| service(&st.key, v))
        .collect();
    if n > SERVICES_SHOWN {
        entries.push(Entry::Separator);
        entries.push(item(
            Cmd::OpenStack(st.key.clone()),
            format!("{} more   ·   Open in doze", n - SERVICES_SHOWN),
        ));
    }
    Entry::Submenu {
        title: format!("Services · {n}"),
        entries,
    }
}

/// One service: "●  api   ·   awake", and its submenu.
fn service(key: &StackKey, v: &words::Instance) -> Entry {
    let look = words::look(v);
    let name = v.name.clone();
    let mut sub = Vec::new();
    match verb_for(v, look) {
        Some((verb, label)) => sub.push(item(Cmd::Service(key.clone(), name.clone(), verb), label)),
        None if look == Look::Paused => {
            sub.push(Entry::Note("enabled = false in doze.hcl".into()));
        }
        None => {}
    }
    let url = crate::palette::web_url(v);
    if !url.is_empty() {
        sub.push(item(Cmd::OpenUrl(url.to_owned()), format!("Open {url}")));
    }
    if !sub.is_empty() {
        sub.push(Entry::Separator);
    }
    sub.push(item(
        Cmd::Instance(key.clone(), name.clone()),
        "Open in doze",
    ));
    sub.push(item(Cmd::Logs(key.clone(), Some(name.clone())), "Logs"));
    Entry::Submenu {
        title: format!("{}  {name}   ·   {}", look.glyph(), look.word()),
        entries: sub,
    }
}

/// The one verb a service's submenu offers for its look: sleep what is up, wake what sleeps, turn on what is off,
/// restart what is in trouble, run a task that is not running. Waking has none: it is on its way.
fn verb_for(v: &words::Instance, look: Look) -> Option<(Verb, &'static str)> {
    if v.kind == Kind::Task {
        return match look {
            Look::Running | Look::Paused => None,
            Look::NotRun => Some((Verb::Task, "Run")),
            _ => Some((Verb::Task, "Run again")),
        };
    }
    match look {
        Look::Awake | Look::Dozing => Some((Verb::Sleep, "Sleep")),
        Look::Asleep => Some((Verb::Wake, "Wake")),
        Look::Off => Some((Verb::On, "Turn on")),
        Look::Crashing | Look::Failed => Some((Verb::Restart, "Restart")),
        Look::Waking | Look::Paused | Look::Running | Look::Done | Look::NotRun => None,
    }
}

/// What the names registry says a host's URL is (`library::Names::url_for`): port-less while the front door is up.
pub type Urls<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// The address to open a running stack at (Go `WebURL`): the first https address, else an app's URL from the names
/// registry, else its name and port.
fn web_url(st: &StackView, names: &Urls<'_>) -> Option<String> {
    if let Some(v) = st.instances.iter().find(|v| !v.https.is_empty()) {
        return Some(v.https.clone());
    }
    st.instances
        .iter()
        .filter(|v| v.kind == "app" && !v.domain.is_empty())
        .find_map(|v| {
            if let Some(url) = names(&v.domain) {
                return Some(url);
            }
            let w = crate::convert::instance(v);
            let r = words::reach(&w)?;
            r.addr
                .starts_with("http")
                .then(|| format!("{}{}", r.addr, r.port.unwrap_or("")))
        })
}

/// What decides a rebuild: the menu with its countdowns left out (Go's signature of the model and `attnSig`). Two
/// menus with the same signature look the same but for a detail's seconds.
#[must_use]
pub fn signature(entries: &[Entry]) -> Vec<Entry> {
    entries.iter().map(masked).collect()
}

fn masked(e: &Entry) -> Entry {
    let quiet = |it: &Item| Item {
        detail: String::new(),
        since: None,
        ..it.clone()
    };
    match e {
        Entry::Detail(_) => Entry::Detail(String::new()),
        Entry::Submenu { title, entries } => Entry::Submenu {
            title: title.clone(),
            entries: signature(entries),
        },
        Entry::Item {
            cmd: Cmd::Fix(fix),
            title,
            key,
        } => Entry::Item {
            cmd: Cmd::Fix(Box::new((quiet(&fix.0), fix.1.clone()))),
            title: title.clone(),
            key: key.clone(),
        },
        Entry::Item {
            cmd: Cmd::Show(it),
            title,
            key,
        } => Entry::Item {
            cmd: Cmd::Show(Box::new(quiet(it))),
            title: title.clone(),
            key: key.clone(),
        },
        other => other.clone(),
    }
}

/// The menu as text, one line an entry, a submenu's lines indented under it: what the native menu shows, for
/// tests and evidence (a status item's menu cannot be captured offscreen).
#[must_use]
pub fn text(entries: &[Entry]) -> String {
    let mut out = String::new();
    write_text(&mut out, entries, 0);
    out
}

fn write_text(out: &mut String, entries: &[Entry], depth: usize) {
    let pad = "    ".repeat(depth);
    for e in entries {
        let line = match e {
            Entry::Item { title, key, .. } if key.is_empty() => title.clone(),
            Entry::Item { title, key, .. } => format!("{title}    {}", key_hint(key)),
            Entry::Note(t) | Entry::Detail(t) => format!("({t})"),
            Entry::Header(t) => format!("[{t}]"),
            Entry::Separator => "────────".into(),
            Entry::Submenu { title, .. } => format!("{title}  ▸"),
        };
        out.push_str(&pad);
        out.push_str(&line);
        out.push('\n');
        if let Entry::Submenu { entries, .. } = e {
            write_text(out, entries, depth + 1);
        }
    }
}

/// "⌘⇧D" for `D`, "⌘," for `,`.
fn key_hint(key: &str) -> String {
    let shifted = key.chars().any(char::is_uppercase);
    format!("⌘{}{}", if shifted { "⇧" } else { "" }, key.to_uppercase())
}
