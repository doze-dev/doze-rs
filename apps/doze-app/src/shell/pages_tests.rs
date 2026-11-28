//! Settings in a gpui test window over a fixture daemon (a throwaway home): its keys, its ways in and out, and that a
//! fixture's Settings acts on nothing on this machine (the login item goes to a temporary folder).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::tests::window;
use super::{Root, Screen};
use crate::settings::Section;

fn settings(
    root: &Entity<Root>,
    vcx: &mut VisualTestContext,
) -> Entity<crate::settings::SettingsView> {
    vcx.update(|_, cx| root.read(cx).pages.settings.clone().unwrap())
}

/// Activity's old keys (⇧A everywhere, ⇧T on a stack's page) are bound to nothing now: the page stays.
#[gpui::test]
fn activitys_old_keys_do_nothing(cx: &mut TestAppContext) {
    let (_src, root, vcx) = window(cx, "shop");
    vcx.run_until_parked();
    let before = vcx.update(|_, cx| root.read(cx).screen.clone());
    assert!(matches!(before, Screen::Stack(_)), "{before:?}");
    vcx.simulate_keystrokes("shift-a shift-t");
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_, cx| root.read(cx).screen.clone()), before);
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
