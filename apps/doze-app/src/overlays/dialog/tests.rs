//! The dialog's keys through gpui itself (a test window, the app's keymap): ←→ move round the buttons, ⏎ presses the
//! highlighted one, `y` answers a restart, and a reset's ⏎ stays on Cancel until the name is typed.

use std::cell::RefCell;
use std::rc::Rc;

use doze_desk::act::{Button, Dialog};
use gpui::{TestAppContext, WindowAppearance};

use super::{DialogEvent, DialogView};
use crate::act::Ask;
use crate::theme::{self, Choice};

fn ready(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        let _ = theme::apply(cx, Choice::default(), WindowAppearance::Dark);
        let keys = crate::actions::keymap(cx);
        cx.bind_keys(keys);
    });
}

fn restart() -> Dialog {
    Dialog {
        kind: "restart".into(),
        needed: true,
        title: "Restart db?".into(),
        buttons: vec![
            Button::run("Restart db", "restart", &["db"]).primary(true),
            Button::cancel(),
        ],
        ..Dialog::default()
    }
}

type Heard = Rc<RefCell<Vec<String>>>;

fn open(
    cx: &mut TestAppContext,
    d: Dialog,
) -> (
    gpui::Entity<DialogView>,
    &mut gpui::VisualTestContext,
    Heard,
) {
    ready(cx);
    let (v, vcx) = cx.add_window_view(|window, cx| {
        DialogView::new(
            Ask {
                key: None,
                dialog: d,
            },
            window,
            cx,
        )
    });
    let heard: Heard = Rc::default();
    let h = heard.clone();
    vcx.update(|_, cx| {
        cx.subscribe(&v, move |_, ev: &DialogEvent, _| {
            h.borrow_mut().push(match ev {
                DialogEvent::Cancel => "cancel".into(),
                DialogEvent::Chose(b, t) => format!("{} {t}", b.label).trim().to_owned(),
            });
        })
        .detach();
    });
    (v, vcx, heard)
}

#[gpui::test]
fn arrows_go_round_and_enter_presses_the_highlighted_button(cx: &mut TestAppContext) {
    let (v, cx, heard) = open(cx, restart());
    cx.simulate_keystrokes("right");
    assert_eq!(v.read_with(cx, |v, _| v.c.at), 1);
    cx.simulate_keystrokes("right");
    assert_eq!(v.read_with(cx, |v, _| v.c.at), 0, "round again");
    cx.simulate_keystrokes("left enter");
    assert_eq!(heard.borrow().as_slice(), ["cancel"]);
}

#[gpui::test]
fn y_answers_a_restart(cx: &mut TestAppContext) {
    let (_, cx, heard) = open(cx, restart());
    cx.simulate_keystrokes("y");
    assert_eq!(heard.borrow().as_slice(), ["Restart db"]);
}

#[gpui::test]
fn a_reset_stays_on_cancel_until_its_name_is_typed(cx: &mut TestAppContext) {
    let reset = Dialog {
        kind: "reset".into(),
        type_name: "db".into(),
        buttons: vec![
            Button {
                danger: true,
                ..Button::run("Reset db", "reset", &["db"])
            },
            Button::cancel(),
        ],
        default: 1,
        ..Dialog::default()
    };
    let (v, cx, heard) = open(cx, reset);
    v.update_in(cx, |v, window, cx| v.type_in("d", window, cx));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        heard.borrow().as_slice(),
        ["cancel"],
        "a wrong name: ⏎ is Cancel"
    );
    v.update_in(cx, |v, window, cx| v.type_in("db", window, cx));
    cx.simulate_keystrokes("enter");
    assert_eq!(heard.borrow().last().map(String::as_str), Some("Reset db"));
}
