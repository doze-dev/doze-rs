//! The menu-bar item and the Dock badge through objc2. Each `unsafe` block sends one Objective-C message whose
//! arguments objc2 types fully; the SAFETY line says why the message is sound at that point.
#![allow(unsafe_code)]

use super::{Icon, Icons, MenuEntry, OnPick, TrayIcon};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBitmapImageRep, NSImage, NSMenu, NSMenuItem, NSScreen, NSStatusBar,
    NSStatusItem,
};
use objc2_foundation::{NSData, NSSize, NSString};
use std::io;

/// The status item's length: as wide as its image (`NSVariableStatusItemLength`).
const VARIABLE_LENGTH: f64 = -1.0;

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

/// A menu line's title without the state mark in front of it ("●  shop" is "shop").
fn bare(title: &str) -> &str {
    title.trim_start_matches(|c: char| !c.is_alphanumeric())
}

fn other(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

/// The template image for `icon`: one representation per scale, each drawn for its pixels, so AppKit picks the 2x
/// one on a Retina screen and the 1x one elsewhere instead of scaling either. Its size in points is the 1x picture's
/// pixels.
fn image(mtm: MainThreadMarker, icon: Icon) -> io::Result<Retained<NSImage>> {
    let one = rep(icon.x1)?;
    let two = rep(icon.x2)?;
    let (w, h) = (one.pixelsWide(), one.pixelsHigh());
    if w <= 0 || h <= 0 || two.pixelsWide() != 2 * w || two.pixelsHigh() != 2 * h {
        return Err(other("tray icon: the 2x picture is not twice the 1x one"));
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "an icon is a few dozen pixels across"
    )]
    let size = NSSize::new(w as f64, h as f64);
    let img = NSImage::initWithSize(mtm.alloc(), size);
    for r in [one, two] {
        r.setSize(size);
        img.addRepresentation(&r);
    }
    img.setTemplate(true);
    Ok(img)
}

fn rep(png: &[u8]) -> io::Result<Retained<NSBitmapImageRep>> {
    NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(png))
        .ok_or_else(|| other("tray icon: not an image"))
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

    pub(super) fn icon_frame(&self) -> Option<[f64; 4]> {
        let window = self.item.button(self.mtm)?.window()?;
        let f = window.frame();
        // Cocoa counts up from the primary screen's bottom; a screen picture counts down from its top.
        let primary = NSScreen::screens(self.mtm).firstObject()?.frame();
        let top = primary.size.height - f.origin.y - f.size.height;
        Some([f.origin.x, top, f.size.width, f.size.height])
    }

    pub(super) fn open_menu(&self) {
        if let Some(button) = self.item.button(self.mtm) {
            // Sent from the run loop's next turn, not from here: the menu then tracks in a loop of its own until it
            // closes, and the caller (the app, mid-update) must not still be on the stack inside that loop.
            let none: Option<&NSObject> = None;
            // SAFETY: a live status bar button on the main thread; `performClick:` takes one object (the sender, here
            // none) and a click with no sender opens its menu. The delay is an NSTimeInterval (f64).
            let () = unsafe {
                msg_send![&*button, performSelector: sel!(performClick:), withObject: none, afterDelay: 0.0_f64]
            };
        }
    }

    /// Closes the menu if it is open.
    pub(super) fn close_menu(&self) {
        if let Some(menu) = self.item.menu(self.mtm) {
            menu.cancelTrackingWithoutAnimation();
        }
    }

    /// The menu AppKit holds now, read back from its items: a line each, indented by depth.
    pub(super) fn menu_text(&self) -> String {
        fn walk(menu: &NSMenu, depth: usize, out: &mut String) {
            for item in menu.itemArray().iter() {
                let pad = "  ".repeat(depth);
                if item.isSeparatorItem() {
                    out.push_str(&format!("{pad}----\n"));
                    continue;
                }
                let mut line = format!("{pad}{}", item.title());
                if item.isSectionHeader() {
                    line = format!("{pad}[{}]", item.title());
                }
                let key = item.keyEquivalent().to_string();
                if !key.is_empty() {
                    line.push_str(&format!("  ⌘{key}"));
                }
                if !item.isEnabled() {
                    line.push_str("  (disabled)");
                } else if item.hasSubmenu() {
                    line.push_str("  >");
                } else if item.action().is_some() {
                    line.push_str(&format!("  #{:x}", item.tag()));
                }
                out.push_str(&line);
                out.push('\n');
                if let Some(sub) = item.submenu() {
                    walk(&sub, depth + 1, out);
                }
            }
        }
        let mut out = String::new();
        if let Some(menu) = self.item.menu(self.mtm) {
            walk(&menu, 0, &mut out);
        }
        out
    }

    /// Chooses the line at `path` (titles from the top menu down, each the line's title, its start after any state mark, or words in it) the way a
    /// click does: the menu sends the line's action to its target (`performActionForItemAtIndex:`). Returns the
    /// line's title, or what was not found.
    pub(super) fn choose(&self, path: &[&str]) -> Result<String, String> {
        let mut menu = self
            .item
            .menu(self.mtm)
            .ok_or_else(|| "the item has no menu".to_owned())?;
        let Some((last, above)) = path.split_last() else {
            return Err("no line named".to_owned());
        };
        let find = |menu: &NSMenu, want: &str| -> Option<(isize, Retained<NSMenuItem>)> {
            let items = menu.itemArray();
            let all: Vec<_> = items.iter().enumerate().collect();
            all.iter()
                .find(|(_, it)| it.title().to_string() == want)
                .or_else(|| {
                    all.iter()
                        .find(|(_, it)| bare(&it.title().to_string()).starts_with(want))
                })
                .or_else(|| {
                    all.iter()
                        .find(|(_, it)| it.title().to_string().contains(want))
                })
                .and_then(|(i, it)| Some((isize::try_from(*i).ok()?, it.clone())))
        };
        for step in above {
            let (_, item) = find(&menu, step).ok_or_else(|| format!("no line {step:?}"))?;
            menu = item
                .submenu()
                .ok_or_else(|| format!("{step:?} has no submenu"))?;
        }
        let (at, item) = find(&menu, last).ok_or_else(|| format!("no line {last:?}"))?;
        if !item.isEnabled() {
            return Err(format!("{last:?} is disabled"));
        }
        menu.performActionForItemAtIndex(at);
        Ok(item.title().to_string())
    }

    /// Which picture the item shows now, and its size in points.
    pub(super) fn shown_icon(&self) -> Option<(TrayIcon, [f64; 2])> {
        let img = self.item.button(self.mtm)?.image()?;
        let which = if Retained::as_ptr(&img) == Retained::as_ptr(&self.alert) {
            TrayIcon::Alert
        } else {
            TrayIcon::Calm
        };
        let s = img.size();
        Some((which, [s.width, s.height]))
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

pub(super) fn dock_badge_now() -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    tile.badgeLabel().map(|s| s.to_string())
}

pub(super) fn ask_notifications() {
    use objc2_foundation::NSBundle;
    use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};
    // The notification centre raises an exception, aborting the process, for a binary outside a bundle.
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let done = block2::RcBlock::new(
        |_granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {},
    );
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &done,
    );
}

pub(super) fn hands_off() -> usize {
    use objc2_app_kit::NSWindowStyleMask;
    let Some(mtm) = MainThreadMarker::new() else {
        return 0;
    };
    let mut n = 0;
    for w in NSApplication::sharedApplication(mtm).windows().iter() {
        // The menu-bar item's own window is borderless; the app's windows have a title bar.
        if w.styleMask().contains(NSWindowStyleMask::Titled) {
            w.setIgnoresMouseEvents(true);
            n += 1;
        }
    }
    n
}

pub(super) fn holds_input() -> (bool, usize) {
    let Some(mtm) = MainThreadMarker::new() else {
        return (false, 0);
    };
    let app = NSApplication::sharedApplication(mtm);
    let key = app.windows().iter().filter(|w| w.isKeyWindow()).count();
    (app.isActive(), key)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_line_is_found_by_its_name_whatever_state_mark_leads_it() {
        assert_eq!(super::bare("●  shop   ·   2 awake"), "shop   ·   2 awake");
        assert_eq!(
            super::bare("⚠︎  shop   ·   api is failed"),
            "shop   ·   api is failed"
        );
        assert_eq!(super::bare("Open doze"), "Open doze");
        assert!(!super::bare("web stopped   ·   shop").starts_with("shop"));
    }
}
