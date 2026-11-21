//! Activity and Settings in a gpui test window over a fixture daemon (a throwaway home): their keys, their ways in and
//! out, and that a fixture's Settings acts on nothing on this machine (the login item goes to a temporary folder).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::tests::window;
use super::{Root, Screen};
use crate::settings::Section;

fn activity(
    root: &Entity<Root>,
    vcx: &mut VisualTestContext,
) -> Entity<crate::activity::ActivityView> {
    vcx.update(|_, cx| root.read(cx).pages.activity.clone().unwrap())
}

fn settings(
    root: &Entity<Root>,
    vcx: &mut VisualTestContext,
) -> Entity<crate::settings::SettingsView> {
    vcx.update(|_, cx| root.read(cx).pages.settings.clone().unwrap())
}

/// Waits for Activity's read of the journal (off the UI thread) to land.
fn landed(root: &Entity<Root>, vcx: &mut VisualTestContext) {
    let start = std::time::Instant::now();
    loop {
        vcx.run_until_parked();
        let a = activity(root, vcx);
        if vcx.update(|_, cx| a.read(cx).model.is_some()) {
            return;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "no activity"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[gpui::test]
fn shift_a_opens_activity_and_its_keys_work(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.simulate_keystrokes("shift-a");
    landed(&root, vcx);
    assert!(vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Activity(_))));
    let a = activity(&root, vcx);
    let rows = vcx.update(|_, cx| a.read(cx).shown.len());
    assert!(rows >= 5, "the shop's services: {rows}");
    // ] is a week; [ [ an hour.
    vcx.simulate_keystrokes("]");
    landed(&root, vcx);
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_, cx| a.read(cx).span),
        doze_desk::activity::Span::Week
    );
    vcx.simulate_keystrokes("[ [");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_, cx| a.read(cx).span),
        doze_desk::activity::Span::Hour
    );
    // j picks the first service, ⏎ shows it alone, esc lets go of it, esc again goes back to the stack.
    vcx.simulate_keystrokes("j enter");
    vcx.run_until_parked();
    let only = vcx.update(|_, cx| a.read(cx).only.clone());
    assert!(!only.is_empty());
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| a.read(cx).only.is_empty()));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Stack(_))));
}

#[gpui::test]
fn the_find_box_narrows_the_rows(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.simulate_keystrokes("shift-a");
    landed(&root, vcx);
    let a = activity(&root, vcx);
    let all = vcx.update(|_, cx| a.read(cx).shown.len());
    vcx.update(|window, cx| a.update(cx, |a, cx| a.find_words("db", window, cx)));
    vcx.run_until_parked();
    let found = vcx.update(|_, cx| a.read(cx).shown.len());
    assert!(found >= 1 && found < all, "{found} of {all}");
}

#[gpui::test]
fn cmd_comma_opens_settings_and_the_arrows_walk_its_parts(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.simulate_keystrokes("cmd-,");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| root.read(cx).screen == Screen::Settings));
    let s = settings(&root, vcx);
    assert_eq!(vcx.update(|_, cx| s.read(cx).section), Section::General);
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_, cx| s.read(cx).section), Section::Machine);
    // A fixture's machine is set up as the docs describe, and leaves nothing behind.
    let (parts, left) = vcx.update(|_, cx| (s.read(cx).parts.clone(), s.read(cx).left.clone()));
    assert!(!parts.is_empty() && parts.iter().all(|p| p.ok), "{parts:?}");
    assert_eq!(left, Some(Vec::new()));
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    let u = vcx.update(|_, cx| s.read(cx).update.clone()).unwrap();
    assert!(u.error.is_empty(), "a fixture asks nobody: {u:?}");
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| matches!(root.read(cx).screen, Screen::Stack(_))));
}

#[gpui::test]
fn a_fixtures_settings_touch_nothing_on_this_machine(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.update(|window, cx| {
        root.update(cx, |r, cx| {
            r.open_settings(Some(Section::Machine), false, window, cx)
        })
    });
    vcx.run_until_parked();
    let s = settings(&root, vcx);
    // The login item is a temporary folder's, never the person's.
    let item = vcx.update(|_, cx| s.read(cx).login_item.clone());
    if let Some(p) = &item {
        assert!(p.starts_with(std::env::temp_dir()), "{p:?}");
        vcx.update(|_, cx| s.update(cx, |s, cx| s.set_login(cx)));
        assert!(p.is_file());
        vcx.update(|_, cx| s.update(cx, |s, cx| s.set_login(cx)));
        assert!(!p.exists());
    }
    // Repair and clean-up say a fixture does nothing, and do nothing.
    vcx.update(|_, cx| {
        s.update(cx, |s, cx| {
            s.repair(cx);
            s.clear("all", cx);
        });
    });
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| !s.read(cx).repairing && s.read(cx).busy.is_empty()));
    let said: Vec<String> = vcx.update(|_, cx| {
        root.read(cx)
            .acts
            .read(cx)
            .notices
            .recent()
            .map(|n| n.text.clone())
            .collect()
    });
    assert!(
        said.iter().any(|t| t.contains("a fixture clears nothing")),
        "{said:?}"
    );
    // Repair opens the run sheet, which shows what would run and runs nothing: a fixture's machine is set up, so
    // there is nothing to do; were there, its Run would say a fixture runs nothing.
    let sheet = vcx.update(|_, cx| root.read(cx).pages.admin.clone());
    let sheet = sheet.unwrap_or_else(|| panic!("Repair opened no run sheet"));
    let phase = vcx.update(|_, cx| sheet.read(cx).phase.clone());
    assert_eq!(
        phase,
        crate::overlays::admin::Phase::Done("nothing to do: every part is in place".into(), false)
    );
}
