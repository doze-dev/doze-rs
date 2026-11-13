//! The menu-bar item, through `doze_os::ui`: a line per stack (● running, ○ stopped, ⚠︎ trouble), Open doze, Search or
//! do…, and Quit, which leaves the stacks running. It follows the hub while the window is hidden (once a second, see
//! `source::HIDDEN`); the menu is rebuilt only when what it says changed, and the bat shows its alert mark while
//! anything is in trouble.

use async_channel::Receiver;
use doze_desk::{Hub, StackKey, StackView};
use doze_display as words;
use doze_os::ui::{MenuEntry, Tray, TrayIcon};

/// What a menu line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cmd {
    Open,
    Palette,
    Quit,
    Stack(StackKey),
}

const OPEN: u32 = 1;
const PALETTE: u32 = 2;
const QUIT: u32 = 3;
/// Stack lines are this plus their place in the list.
const STACKS: u32 = 100;
/// Running stacks listed, then stopped ones (Go's 8 and 5).
const RUNNING_SHOWN: usize = 8;
const STOPPED_SHOWN: usize = 5;

/// The item and what it last showed.
pub(crate) struct Menu {
    tray: Tray,
    shown: Vec<MenuEntry>,
    keys: Vec<StackKey>,
    icon: TrayIcon,
}

/// One stack's line and whether it is in trouble.
fn line(v: &StackView) -> (String, bool) {
    if !v.running {
        return (format!("○  {}  ·  stopped", v.name), false);
    }
    let t = words::tally(&v.words().instances);
    if t.trouble > 0 {
        return (format!("⚠︎  {}  ·  {} in trouble", v.name, t.trouble), true);
    }
    (format!("●  {}  ·  {} awake", v.name, t.awake), false)
}

/// The menu for the stacks the hub knows, the stacks its lines open, and whether anything is in trouble.
pub(crate) fn entries(stacks: &[StackView]) -> (Vec<MenuEntry>, Vec<StackKey>, bool) {
    let mut out = Vec::new();
    let mut keys = Vec::new();
    let mut trouble = false;
    let (running, stopped): (Vec<&StackView>, Vec<&StackView>) =
        stacks.iter().partition(|s| s.running);
    for v in running
        .iter()
        .take(RUNNING_SHOWN)
        .chain(stopped.iter().take(STOPPED_SHOWN))
    {
        let (text, bad) = line(v);
        trouble |= bad;
        out.push(MenuEntry::item(
            STACKS + u32::try_from(keys.len()).unwrap_or(0),
            &text,
            "",
        ));
        keys.push(v.key.clone());
    }
    let more =
        running.len().saturating_sub(RUNNING_SHOWN) + stopped.len().saturating_sub(STOPPED_SHOWN);
    if more > 0 {
        out.push(MenuEntry::Note(format!("More stacks · {more}")));
    }
    if stacks.is_empty() {
        out.push(MenuEntry::Note("No stacks yet".into()));
    }
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::item(OPEN, "Open doze", "D"));
    out.push(MenuEntry::item(PALETTE, "Search or do…", "k"));
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::item(
        QUIT,
        "Quit doze (stacks keep running)",
        "q",
    ));
    (out, keys, trouble)
}

impl Menu {
    /// Puts the item in the menu bar. `None` where there is no menu bar item (Linux, until its milestone).
    pub(crate) fn install(hub: &Hub) -> Option<(Menu, Receiver<u32>)> {
        let (tx, rx) = async_channel::unbounded();
        let (shown, keys, trouble) = entries(&hub.stacks());
        let tray = Tray::install(
            crate::assets::TRAY,
            "doze",
            &shown,
            Box::new(move |id| {
                let _ = tx.try_send(id);
            }),
        )
        .ok()?;
        let icon = if trouble {
            TrayIcon::Alert
        } else {
            TrayIcon::Calm
        };
        tray.set_icon(icon);
        Some((
            Menu {
                tray,
                shown,
                keys,
                icon,
            },
            rx,
        ))
    }

    /// Follows the hub: rebuilds the menu only if it would say something else.
    pub(crate) fn follow(&mut self, hub: &Hub) {
        let (shown, keys, trouble) = entries(&hub.stacks());
        if shown != self.shown {
            self.tray.set_menu(&shown);
            self.shown = shown;
            self.keys = keys;
        }
        let icon = if trouble {
            TrayIcon::Alert
        } else {
            TrayIcon::Calm
        };
        if icon != self.icon {
            self.tray.set_icon(icon);
            self.icon = icon;
        }
        doze_os::ui::dock_badge(trouble.then_some("!"));
    }

    /// What a picked line asks for.
    pub(crate) fn cmd(&self, id: u32) -> Option<Cmd> {
        match id {
            OPEN => Some(Cmd::Open),
            PALETTE => Some(Cmd::Palette),
            QUIT => Some(Cmd::Quit),
            n => self
                .keys
                .get(usize::try_from(n.checked_sub(STACKS)?).ok()?)
                .cloned()
                .map(Cmd::Stack),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::entries;
    use doze_desk::{StackKey, StackView};
    use doze_os::ui::MenuEntry;

    #[test]
    fn stacks_list_running_first_and_the_fixed_lines_follow() {
        let mut a = StackView::new(StackKey::new("/h/projects/a"));
        a.name = "shop".into();
        a.running = true;
        let mut b = StackView::new(StackKey::new("/h/projects/b"));
        b.name = "larder".into();
        let (menu, keys, trouble) = entries(&[a, b]);
        assert!(!trouble);
        assert_eq!(keys.len(), 2);
        let titles: Vec<String> = menu
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Item { title, .. } => Some(title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            titles.first().map(String::as_str),
            Some("●  shop  ·  0 awake")
        );
        assert!(titles.iter().any(|t| t.starts_with("Quit doze")));
    }
}
