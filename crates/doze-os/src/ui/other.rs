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
}

pub(super) fn dock_badge(_: Option<&str>) {}
