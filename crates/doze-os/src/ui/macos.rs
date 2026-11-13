//! The menu-bar item and the Dock badge through objc2. Each `unsafe` block sends one Objective-C message whose
//! arguments objc2 types fully; the SAFETY line says why the message is sound at that point.
#![allow(unsafe_code)]

use super::{Icons, MenuEntry, OnPick, TrayIcon};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSData, NSSize, NSString};
use std::io;

/// The status item's length: as wide as its image (`NSVariableStatusItemLength`).
const VARIABLE_LENGTH: f64 = -1.0;
/// The bar's image size in points.
const ICON_POINTS: f64 = 22.0;

pub(super) struct Ivars {
    on_pick: OnPick,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class adds no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DozeTrayTarget"]
    #[ivars = Ivars]
    pub(super) struct Target;

    impl Target {
        // SAFETY: the selector `pick:` takes one object argument (the menu item that sent it) and returns nothing.
        #[unsafe(method(pick:))]
        fn pick(&self, sender: Option<&NSMenuItem>) {
            if let Some(item) = sender
                && let Ok(id) = u32::try_from(item.tag())
            {
                (self.ivars().on_pick)(id);
            }
        }
    }

    // SAFETY: NSObjectProtocol has no requirements beyond NSObject's.
    unsafe impl NSObjectProtocol for Target {}
);

impl Target {
    fn new(mtm: MainThreadMarker, on_pick: OnPick) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Ivars { on_pick });
        // SAFETY: `init` is NSObject's designated initialiser, called once on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

pub(super) struct Tray {
    mtm: MainThreadMarker,
    item: Retained<NSStatusItem>,
    target: Retained<Target>,
    calm: Retained<NSImage>,
    alert: Retained<NSImage>,
}

fn other(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

fn image(mtm: MainThreadMarker, png: &[u8]) -> io::Result<Retained<NSImage>> {
    let data = NSData::with_bytes(png);
    let img = NSImage::initWithData(mtm.alloc(), &data)
        .ok_or_else(|| other("tray icon: not an image"))?;
    img.setSize(NSSize::new(ICON_POINTS, ICON_POINTS));
    img.setTemplate(true);
    Ok(img)
}

impl Tray {
    pub(super) fn install(
        icons: Icons,
        tooltip: &str,
        menu: &[MenuEntry],
        on_pick: OnPick,
    ) -> io::Result<Tray> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| other("the menu-bar item is made on the main thread"))?;
        let calm = image(mtm, icons.calm)?;
        let alert = image(mtm, icons.alert)?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(VARIABLE_LENGTH);
        if let Some(button) = item.button(mtm) {
            button.setImage(Some(&calm));
            button.setToolTip(Some(&NSString::from_str(tooltip)));
        }
        let tray = Tray {
            mtm,
            item,
            target: Target::new(mtm, on_pick),
            calm,
            alert,
        };
        tray.set_menu(menu);
        Ok(tray)
    }

    pub(super) fn set_menu(&self, entries: &[MenuEntry]) {
        self.item.setMenu(Some(&self.menu(entries)));
    }

    /// A menu of `entries`, its submenus made the same way.
    fn menu(&self, entries: &[MenuEntry]) -> Retained<NSMenu> {
        let menu = NSMenu::new(self.mtm);
        menu.setAutoenablesItems(false);
        for e in entries {
            let item = match e {
                MenuEntry::Separator => NSMenuItem::separatorItem(self.mtm),
                MenuEntry::Note(title) => {
                    let it = self.line(title, None, "");
                    it.setEnabled(false);
                    it
                }
                MenuEntry::Header(title) => {
                    NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), self.mtm)
                }
                MenuEntry::Submenu { title, entries } => {
                    let it = self.line(title, None, "");
                    let sub = self.menu(entries);
                    sub.setTitle(&NSString::from_str(title));
                    it.setSubmenu(Some(&sub));
                    it
                }
                MenuEntry::Item { id, title, key } => {
                    let it = self.line(title, Some(sel!(pick:)), key);
                    it.setTag(isize::try_from(*id).unwrap_or(isize::MAX));
                    // SAFETY: the target is a live Target that answers `pick:`; the tray keeps it for as long as the
                    // menu can send to it (the menu holds it weakly, as AppKit targets are).
                    unsafe { it.setTarget(Some(&self.target)) };
                    it
                }
            };
            menu.addItem(&item);
        }
        menu
    }

    pub(super) fn set_tooltip(&self, tooltip: &str) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setToolTip(Some(&NSString::from_str(tooltip)));
        }
    }

    pub(super) fn open_menu(&self) {
        if let Some(button) = self.item.button(self.mtm) {
            // SAFETY: a live status bar button on the main thread; a click with no sender opens its menu.
            unsafe { button.performClick(None) };
        }
    }

    fn line(&self, title: &str, action: Option<Sel>, key: &str) -> Retained<NSMenuItem> {
        // SAFETY: a freshly allocated item initialised once with a title, an optional action and a key equivalent.
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                self.mtm.alloc(),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        }
    }

    pub(super) fn set_icon(&self, icon: TrayIcon) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setImage(Some(match icon {
                TrayIcon::Calm => &self.calm,
                TrayIcon::Alert => &self.alert,
            }));
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

pub(super) fn dock_badge(label: Option<&str>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    tile.setBadgeLabel(label.map(NSString::from_str).as_deref());
}
