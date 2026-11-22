//! The help sheet in a real window over a fixture daemon: it opens on the page's keys, the filter finds a key of
//! another context by its words, and esc clears the filter first and closes the sheet second.
#![allow(clippy::unwrap_used)]

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::tests::window;
use super::{Root, Screen};
use crate::actions::{Scope, ShowHelp};
use crate::overlays::Overlay;
use crate::overlays::help::model::Section;
use crate::overview::View;

/// Opens the sheet as `?` does, and says what it shows.
fn open(root: &Entity<Root>, vcx: &mut VisualTestContext) -> Section {
    vcx.update(|window, cx| root.update(cx, |r, cx| r.on_help(&ShowHelp, window, cx)));
    vcx.run_until_parked();
    shown(root, vcx).unwrap()
}

fn shown(root: &Entity<Root>, vcx: &mut VisualTestContext) -> Option<Section> {
    vcx.update(|_, cx| match &root.read(cx).overlay {
        Some(Overlay::Help(h)) => Some(h.read(cx).on),
        _ => None,
    })
}

fn close(root: &Entity<Root>, vcx: &mut VisualTestContext) {
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(shown(root, vcx), None);
}

#[gpui::test]
fn the_sheet_opens_on_the_current_pages_keys(cx: &mut TestAppContext) {
    let (src, root, vcx) = window(cx, "shop");
    let key = src.first.clone().unwrap();
    vcx.update(|window, cx| root.update(cx, |r, cx| r.go(Screen::Library, window, cx)));
    vcx.run_until_parked();
    assert_eq!(open(&root, vcx), Section::Keys(Scope::Library));
    close(&root, vcx);
    vcx.update(|window, cx| {
        root.update(cx, |r, cx| {
            r.go(Screen::Stack(key.clone()), window, cx);
            r.overview.update(cx, |o, cx| o.set_view(View::Tiles, cx));
        });
    });
    vcx.run_until_parked();
    assert_eq!(open(&root, vcx), Section::Keys(Scope::Overview));
    close(&root, vcx);
    vcx.update(|_, cx| {
        root.update(cx, |r, cx| {
            r.overview.update(cx, |o, cx| o.set_view(View::Map, cx))
        })
    });
    vcx.run_until_parked();
    assert_eq!(open(&root, vcx), Section::Keys(Scope::Map));
    close(&root, vcx);
    vcx.update(|window, cx| root.update(cx, |r, cx| r.open_logs(window, cx)));
    vcx.run_until_parked();
    assert_eq!(open(&root, vcx), Section::Keys(Scope::Logs));
    // ⌘/ again closes it.
    vcx.simulate_keystrokes("cmd-/");
    vcx.run_until_parked();
    assert_eq!(shown(&root, vcx), None);
}

#[gpui::test]
fn esc_clears_the_filter_first_and_closes_the_sheet_second(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.update(|window, cx| root.update(cx, |r, cx| r.go(Screen::Library, window, cx)));
    vcx.run_until_parked();
    assert_eq!(open(&root, vcx), Section::Keys(Scope::Library));
    // Typed into the sheet's own box: a Map key, found from Home by what it does.
    vcx.simulate_input("neighbour");
    vcx.run_until_parked();
    let found = vcx.update(|_, cx| match &root.read(cx).overlay {
        Some(Overlay::Help(h)) => {
            let h = h.read(cx);
            (
                h.query(cx),
                h.found.count(Section::Keys(Scope::Map)),
                h.found.keys.len(),
            )
        }
        _ => (String::new(), 0, 0),
    });
    assert_eq!(found, ("neighbour".to_owned(), 1, 1));
    // ↓ does not move the rail while the matches are shown.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(shown(&root, vcx), Some(Section::Keys(Scope::Library)));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    let left = vcx.update(|_, cx| match &root.read(cx).overlay {
        Some(Overlay::Help(h)) => Some((h.read(cx).query(cx), h.read(cx).found.is_empty())),
        _ => None,
    });
    assert_eq!(
        left,
        Some((String::new(), true)),
        "esc cleared the filter, the sheet stays"
    );
    // With nothing typed, ↓ moves through the rail.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(shown(&root, vcx), Some(Section::Keys(Scope::Overview)));
    vcx.simulate_keystrokes("up up up");
    vcx.run_until_parked();
    assert_eq!(shown(&root, vcx), Some(Section::Keys(Scope::Global)));
    close(&root, vcx);
    // The page under it is still Home: esc closed the sheet, not the page.
    let home = vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Library));
    assert!(home);
}
