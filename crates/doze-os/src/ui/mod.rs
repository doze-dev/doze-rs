//! The desktop app's native surface, which GPUI does not draw: the menu-bar item (a status item with its menu) and the
//! Dock badge (docs/port/app/PLAN.md §2.1 rows 1 and 5, §3 item 16). Behind the `ui` feature, so only the app links
//! AppKit.
//!
//! macOS goes through objc2 (`NSStatusItem`, `NSMenu`, `NSDockTile`); each `unsafe` call is one Objective-C message
//! with a SAFETY line. Other systems have no status item here yet (Linux gets a StatusNotifierItem with the Linux
//! milestone); there [`Tray::install`] says `Unsupported` and the Dock badge does nothing.
//!
//! Everything here runs on the main thread: AppKit requires it, and a call from another thread is an error rather
//! than undefined behaviour.
//!
//! What GPUI already does is not repeated here: activation policy, `doze://` URL events (`Application::on_open_urls`),
//! the app menus and the Dock menu.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(target_os = "macos"))]
use other as imp;

use std::fmt;
use std::io;

/// One line of the menu-bar item's menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// A command: `id` comes back to the handler when it is chosen; `key` is its key equivalent (`"d"` for ⌘D,
    /// uppercase for ⌘⇧), empty for none.
    Item {
        /// What the handler is given.
        id: u32,
        /// The words on the line.
        title: String,
        /// The key equivalent.
        key: String,
    },
    /// A line with no command: a fact, drawn dimmed.
    Note(String),
    /// A section header (macOS 14 draws it small and grey above its group).
    Header(String),
    /// A separator.
    Separator,
    /// A line that opens a menu of its own.
    Submenu {
        /// The words on the line.
        title: String,
        /// Its menu.
        entries: Vec<MenuEntry>,
    },
}

impl MenuEntry {
    /// A command line.
    #[must_use]
    pub fn item(id: u32, title: &str, key: &str) -> Self {
        MenuEntry::Item {
            id,
            title: title.to_owned(),
            key: key.to_owned(),
        }
    }
}

/// Which picture the menu-bar item shows: doze's bat, or the bat with the alert mark (something is red).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIcon {
    /// All well.
    Calm,
    /// Something needs you.
    Alert,
}

/// Template images for the two icons. The caller embeds them; macOS tints a template image to match the menu bar.
#[derive(Debug, Clone, Copy)]
pub struct Icons {
    /// [`TrayIcon::Calm`].
    pub calm: Icon,
    /// [`TrayIcon::Alert`].
    pub alert: Icon,
}

/// One icon as PNG, drawn for each scale: `x1`'s pixels are the icon's size in points (18 tall for the menu bar),
/// `x2` is the same picture at twice as many pixels, drawn at that size rather than scaled.
#[derive(Debug, Clone, Copy)]
pub struct Icon {
    /// At 1x.
    pub x1: &'static [u8],
    /// At 2x.
    pub x2: &'static [u8],
}

/// What runs when a menu line is chosen: its `id`. Called on the main thread.
pub type OnPick = Box<dyn Fn(u32) + 'static>;

/// The menu-bar item. It stays in the bar while this value lives; dropping it removes the item.
pub struct Tray {
    inner: imp::Tray,
}

impl fmt::Debug for Tray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tray").finish_non_exhaustive()
    }
}

impl Tray {
    /// Puts the item in the menu bar with `menu`, calling `on_pick` with a line's id when it is chosen.
    ///
    /// # Errors
    ///
    /// `Unsupported` where there is no status item yet; `Other` off the main thread or when the image will not load.
    pub fn install(
        icons: Icons,
        tooltip: &str,
        menu: &[MenuEntry],
        on_pick: OnPick,
    ) -> io::Result<Tray> {
        imp::Tray::install(icons, tooltip, menu, on_pick).map(|inner| Tray { inner })
    }

    /// Replaces the menu. The app calls this only when what the menu says changed.
    pub fn set_menu(&self, menu: &[MenuEntry]) {
        self.inner.set_menu(menu);
    }

    /// Switches the picture.
    pub fn set_icon(&self, icon: TrayIcon) {
        self.inner.set_icon(icon);
    }

    /// The words shown on a hover ("doze · 4 awake").
    pub fn set_tooltip(&self, tooltip: &str) {
        self.inner.set_tooltip(tooltip);
    }

    /// Where the item sits on screen, in points from the primary screen's top left: `[x, y, width, height]` (for a
    /// picture of it). `None` where there is no item.
    #[must_use]
    pub fn icon_frame(&self) -> Option<[f64; 4]> {
        self.inner.icon_frame()
    }

    /// Opens the menu as a click on the item would, on the run loop's next turn (for a picture of it; the menu tracks
    /// until it is closed).
    pub fn open_menu(&self) {
        self.inner.open_menu();
    }

    /// Closes the menu if it is open.
    pub fn close_menu(&self) {
        self.inner.close_menu();
    }

    /// The menu as AppKit holds it now, read back from its items (not from what was handed to [`Tray::set_menu`]): a
    /// line each, indented by depth; `[Header]`, `----` for a separator, `>` for a submenu, `(disabled)` or the line's id in hex.
    #[must_use]
    pub fn menu_text(&self) -> String {
        self.inner.menu_text()
    }

    /// Chooses the line at `path` (titles from the top menu down; each the line's title, its start after any state mark, or words in it) the way a
    /// click on it does: the menu sends the line's action to its target, so `on_pick` hears it as it hears a click.
    /// No pointer and no Accessibility: for checking every line against a real daemon.
    ///
    /// # Errors
    ///
    /// What was not found, or that the line is disabled.
    pub fn choose(&self, path: &[&str]) -> Result<String, String> {
        self.inner.choose(path)
    }

    /// Which picture the item shows now and its size in points, read back from its button.
    #[must_use]
    pub fn shown_icon(&self) -> Option<(TrayIcon, [f64; 2])> {
        self.inner.shown_icon()
    }
}

/// Sets the Dock icon's badge (`Some("3")`), or clears it (`None`). Does nothing where there is no Dock, and while the
/// app has no Dock icon (menu bar only), the badge waits for the icon.
pub fn dock_badge(label: Option<&str>) {
    imp::dock_badge(label);
}

/// The Dock badge as the Dock tile holds it now (read back, for evidence that it was taken); `None` when there is none.
#[must_use]
pub fn dock_badge_now() -> Option<String> {
    imp::dock_badge_now()
}

/// Asks the system, once, for leave to post notifications (Alert and Sound), so the first one is not lost to the
/// question. Only from an app bundle (the notification centre refuses a bare binary); a no-op elsewhere and when the
/// person already answered.
pub fn ask_notifications() {
    imp::ask_notifications();
}

/// Hands off (a test, a shot, the bench or a scripted run): every titled window of this app lets the pointer through
/// to whatever is under it, so a person's clicks on the screen never reach it. Returns how many windows it set. A
/// no-op where there is no AppKit.
pub fn hands_off() -> usize {
    imp::hands_off()
}

/// Gives the heap's free pages back to the system (`malloc_zone_pressure_relief`): after the window closes, what it
/// freed (its views, text layouts, a page's log lines) stays counted against the app until the allocator returns it.
/// Returns how many bytes went back. A no-op where there is no such call.
pub fn give_back_memory() -> usize {
    imp::give_back_memory()
}

/// A measuring run (`doze-app bench --on-top`): every titled window of this app floats over the others, on every
/// Space and over a full-screen app, and is brought to the front without activating the app, so the system counts it
/// on screen (a covered window is sent no frames, and its reading would be a hidden one's). Returns how many windows
/// it set. A no-op where there is no AppKit.
pub fn float_over() -> usize {
    imp::float_over()
}

/// A measuring run (`doze-app bench --churn`): the app unhidden and every titled window of it brought back from the
/// Dock and to the front, all without activating the app (the person keeps the keyboard). Returns how many windows
/// it brought back. A no-op where there is no AppKit.
pub fn bring_back() -> usize {
    imp::bring_back()
}

/// Whether this app holds the person's input: it is the active app (it gets the keyboard), and how many of its
/// windows are key. A hands-off run checks both are none.
#[must_use]
pub fn holds_input() -> (bool, usize) {
    imp::holds_input()
}
