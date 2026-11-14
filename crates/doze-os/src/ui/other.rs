//! No status item yet off macOS (Linux gets one with its milestone: a StatusNotifierItem).

use super::{Icons, MenuEntry, OnPick, TrayIcon};
use std::io;

pub(super) struct Tray;

impl Tray {
    pub(super) fn install(_: Icons, _: &str, _: &[MenuEntry], _: OnPick) -> io::Result<Tray> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no menu-bar item on this system yet",
        ))
    }

    pub(super) fn set_menu(&self, _: &[MenuEntry]) {}

    pub(super) fn set_icon(&self, _: TrayIcon) {}

    pub(super) fn set_tooltip(&self, _: &str) {}

    pub(super) fn icon_frame(&self) -> Option<[f64; 4]> {
        None
    }

    pub(super) fn open_menu(&self) {}
}

pub(super) fn dock_badge(_: Option<&str>) {}

pub(super) fn dock_badge_now() -> Option<String> {
    None
}

pub(super) fn ask_notifications() {}

pub(super) fn hands_off() -> usize {
    0
}

pub(super) fn holds_input() -> (bool, usize) {
    (false, 0)
}
