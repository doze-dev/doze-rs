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
    /// A line with no command: a heading or a fact, drawn dimmed.
    Note(String),
    /// A separator.
    Separator,
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

/// Template images for the two icons, PNG at 2x (44 × 44 for a 22 pt bar). The caller embeds them; macOS tints a
/// template image to match the menu bar.
#[derive(Debug, Clone, Copy)]
pub struct Icons {
    /// [`TrayIcon::Calm`].
    pub calm: &'static [u8],
    /// [`TrayIcon::Alert`].
    pub alert: &'static [u8],
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
}

/// Sets the Dock icon's badge (`Some("3")`), or clears it (`None`). Does nothing where there is no Dock, and while the
/// app has no Dock icon (menu bar only), the badge waits for the icon.
pub fn dock_badge(label: Option<&str>) {
    imp::dock_badge(label);
}
