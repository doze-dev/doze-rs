//! The overlays' keys, driven through gpui's test context: the keystrokes resolve through the app's own keymap (the
//! action table), so these prove the bindings and the handlers together.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use doze_desk::attention::{Attention, Fix, FixKind, Item};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::bell::{Bell, BellEvent};
use super::palette::{Palette, PaletteEvent};

fn boot(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        let _ = crate::theme::apply(
            cx,
            crate::theme::Choice::default(),
            gpui::WindowAppearance::Dark,
        );
        let keys = crate::actions::keymap(cx);
        cx.bind_keys(keys);
    });
}

fn item(id: &str, red: bool) -> Item {
    Item {
        id: id.into(),
        print: format!("{id}-said"),
        red,
        kind: "instance",
        title: format!("{id} keeps crashing"),
        fixes: vec![Fix {
            label: "Restart".into(),
            kind: FixKind::Do,
            verb: "restart",
            primary: true,
            ..Fix::default()
        }],
        ..Item::default()
    }
}

#[gpui::test]
fn bell_keys_move_fix_show_and_put_aside(cx: &mut TestAppContext) {
    boot(cx);
    let list = Attention {
        red: 2,
        yellow: 0,
        items: vec![item("a", true), item("b", true)],
        hidden: 0,
    };
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let (bell, cx) =
        cx.add_window_view(|_, cx| Bell::new(list, UNIX_EPOCH + Duration::from_secs(9), cx));
    let s = seen.clone();
    cx.update(|window, cx| {
        cx.subscribe(&bell, move |_, ev: &BellEvent, _| {
            s.borrow_mut().push(match ev {
                BellEvent::Fix(i, f) => format!("fix {} {}", i.id, f.label),
                BellEvent::Show(i) => format!("show {}", i.id),
                BellEvent::NotNow(i) => format!("notnow {}", i.id),
            });
        })
        .detach();
        window.focus(&bell.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("enter down cmd-enter n up up n");
    assert_eq!(
        *seen.borrow(),
        ["fix a Restart", "show b", "notnow b", "notnow a"]
    );
}

fn palette(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<Palette>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<String>>>,
) {
    boot(cx);
    let home = tempfile::tempdir().unwrap();
    let hub = doze_desk::Hub::new(doze_desk::Options::new(home.path()));
    std::mem::forget(home);
    let (p, cx) = cx
        .add_window_view(|window, cx| Palette::new(hub, String::new(), String::new(), window, cx));
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let s = seen.clone();
    cx.update(|_, cx| {
        cx.subscribe(&p, move |_, ev: &PaletteEvent, _| {
            let PaletteEvent::Do(d) = ev;
            s.borrow_mut().push(format!("{:?} {}", d.kind, d.text));
        })
        .detach();
    });
    (p, cx, seen)
}

#[gpui::test]
fn palette_finds_moves_and_does(cx: &mut TestAppContext) {
    let (p, cx, seen) = palette(cx);
    cx.simulate_input("theme te");
    cx.update(|_, cx| {
        let m = &p.read(cx).model;
        assert_eq!(
            m.rows().next().map(|r| r.label.as_str()),
            Some("Theme: teal")
        );
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(*seen.borrow(), ["App theme:teal"]);
    // Esc (the root's CloseOverlay asks the palette first) clears the box; the palette stays, at hand again.
    cx.update(|window, cx| assert!(p.update(cx, |p, cx| p.escape(window, cx))));
    cx.update(|_, cx| {
        let pl = p.read(cx);
        assert!(pl.input.read(cx).value().is_empty());
        assert!(pl.model.sections.iter().any(|s| s.title == "Stacks"));
    });
}

#[gpui::test]
fn palette_says_nothing_and_backspace_types(cx: &mut TestAppContext) {
    let (p, cx, _) = palette(cx);
    cx.simulate_input("zzzq");
    cx.simulate_keystrokes("backspace");
    cx.update(|_, cx| {
        let pl = p.read(cx);
        // Not in a scope: ⌫ is the box's own.
        assert_eq!(pl.input.read(cx).value().as_ref(), "zzz");
        assert!(pl.model.nothing.is_some());
    });
}
