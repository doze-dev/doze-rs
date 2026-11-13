//! The menu-bar item, through `doze_os::ui`: the menu `doze_desk::tray` describes (Needs you, a line per stack with its
//! submenu, the app's own lines), the bat with its alert mark while anything is red, the hover line. The menu is
//! rebuilt only when its signature changes (a countdown in a detail does not count); the item follows the hub while
//! the window is closed too (once a second, `source::HIDDEN`).
//!
//! A line's id is the menu's generation and the line's place, so a pick that arrives after a rebuild is dropped
//! rather than taken for the line now in its place.

use std::time::SystemTime;

use async_channel::Receiver;
use doze_desk::StackView;
use doze_desk::attention::Attention;
use doze_desk::tray::{self, Cmd, Entry};
use doze_os::ui::{MenuEntry, Tray, TrayIcon};

/// The item, what it shows, and what its lines ask for.
pub(crate) struct Menu {
    tray: Tray,
    signature: Vec<Entry>,
    cmds: Vec<Cmd>,
    generation: u16,
    icon: TrayIcon,
    tooltip: String,
}

/// A line's id: the generation in the high half, its place in the low.
fn id_of(generation: u16, place: usize) -> u32 {
    (u32::from(generation) << 16) | u32::try_from(place & 0xffff).unwrap_or(0)
}

/// The native entries for `entries`, each command's line numbered by its place in `cmds`.
fn native(entries: &[Entry], generation: u16, cmds: &mut Vec<Cmd>) -> Vec<MenuEntry> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Item { cmd, title, key } => {
                cmds.push(cmd.clone());
                MenuEntry::item(id_of(generation, cmds.len() - 1), title, key)
            }
            Entry::Note(t) | Entry::Detail(t) => MenuEntry::Note(t.clone()),
            Entry::Header(t) => MenuEntry::Header(t.clone()),
            Entry::Separator => MenuEntry::Separator,
            Entry::Submenu { title, entries } => MenuEntry::Submenu {
                title: title.clone(),
                entries: native(entries, generation, cmds),
            },
        })
        .collect()
}

/// The menu as data, for the stacks and the list at `now`; with the alert mark and the hover line.
pub(crate) fn model(
    stacks: &[StackView],
    list: &Attention,
    now: SystemTime,
) -> (Vec<Entry>, TrayIcon, String) {
    let mut m = tray::build(stacks, now);
    // The mark is what is red and not put aside, as the bell counts it.
    m.needs_you = list.red > 0;
    let icon = if m.needs_you {
        TrayIcon::Alert
    } else {
        TrayIcon::Calm
    };
    (tray::menu(&m, list, stacks), icon, m.tooltip)
}

impl Menu {
    /// Puts the item in the menu bar. `None` where there is no menu-bar item (Linux, until its milestone).
    pub(crate) fn install(stacks: &[StackView], now: SystemTime) -> Option<(Menu, Receiver<u32>)> {
        let (tx, rx) = async_channel::unbounded();
        let (entries, icon, tooltip) = model(stacks, &Attention::default(), now);
        let mut cmds = Vec::new();
        let shown = native(&entries, 0, &mut cmds);
        let tray = Tray::install(
            crate::assets::TRAY,
            &tooltip,
            &shown,
            Box::new(move |id| {
                let _ = tx.try_send(id);
            }),
        )
        .ok()?;
        tray.set_icon(icon);
        write_text(&entries);
        let m = Menu {
            tray,
            signature: tray::signature(&entries),
            cmds,
            generation: 0,
            icon,
            tooltip,
        };
        Some((m, rx))
    }

    /// Follows the hub and Needs you: rebuilds the menu only if it would say something else.
    pub(crate) fn follow(&mut self, stacks: &[StackView], list: &Attention, now: SystemTime) {
        let (entries, icon, tooltip) = model(stacks, list, now);
        let sig = tray::signature(&entries);
        if sig != self.signature {
            self.generation = self.generation.wrapping_add(1);
            let mut cmds = Vec::new();
            let shown = native(&entries, self.generation, &mut cmds);
            self.tray.set_menu(&shown);
            self.signature = sig;
            self.cmds = cmds;
            write_text(&entries);
        }
        if icon != self.icon {
            self.tray.set_icon(icon);
            self.icon = icon;
        }
        if tooltip != self.tooltip {
            self.tray.set_tooltip(&tooltip);
            self.tooltip = tooltip;
        }
    }

    /// What a picked line asks for; `None` for a line of a menu since replaced.
    pub(crate) fn cmd(&self, id: u32) -> Option<Cmd> {
        if id >> 16 != u32::from(self.generation) {
            return None;
        }
        self.cmds.get(usize::try_from(id & 0xffff).ok()?).cloned()
    }

    /// Opens the menu (for a picture of it).
    pub(crate) fn open(&self) {
        self.tray.open_menu();
    }
}

/// The menu as text, to `$DOZE_APP_TRAY_TEXT` when it is set: what the native menu shows, as evidence a status
/// item's menu cannot be shot offscreen.
fn write_text(entries: &[Entry]) {
    if let Some(path) = std::env::var_os("DOZE_APP_TRAY_TEXT") {
        let _ = std::fs::write(path, tray::text(entries));
    }
}

#[cfg(test)]
mod tests {
    use super::{id_of, native};
    use doze_desk::tray::{Cmd, Entry};
    use doze_os::ui::MenuEntry;

    #[test]
    fn ids_carry_the_generation_and_submenus_number_on() {
        let entries = vec![
            Entry::Item {
                cmd: Cmd::OpenDoze,
                title: "Open doze".into(),
                key: "D".into(),
            },
            Entry::Submenu {
                title: "shop".into(),
                entries: vec![Entry::Item {
                    cmd: Cmd::Quit,
                    title: "Quit".into(),
                    key: String::new(),
                }],
            },
        ];
        let mut cmds = Vec::new();
        let out = native(&entries, 3, &mut cmds);
        assert_eq!(cmds, vec![Cmd::OpenDoze, Cmd::Quit]);
        assert_eq!(
            out.first(),
            Some(&MenuEntry::item(id_of(3, 0), "Open doze", "D"))
        );
        let inner = match out.get(1) {
            Some(MenuEntry::Submenu { entries, .. }) => entries.first().cloned(),
            _ => None,
        };
        assert_eq!(inner, Some(MenuEntry::item(id_of(3, 1), "Quit", "")));
    }
}
