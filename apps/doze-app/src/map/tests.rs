//! The Map for 20–150 services wired end to end, in a gpui test window over a fixture daemon (`medium-150`, a
//! throwaway home): find flies to the card and lands on it (at once with motion reduced), `H` opens a neighbourhood
//! and steps its hops, `L` draws every line, `T` shows only trouble, and esc comes back where the camera was.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{Lens, MapView};
use crate::overview::View;
use crate::shell::Root;
use crate::source::Source;

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

/// The Map of `fixture`, laid out, its camera sized as a 1440 × 800 drawing, the keys on it.
fn map<'a>(
    cx: &'a mut TestAppContext,
    fixture: &str,
) -> (Source, Entity<MapView>, &'a mut VisualTestContext) {
    boot(cx);
    let src = Source::fixture(fixture, None).unwrap();
    src.hub.set_visible(true);
    let key = src.first.clone().unwrap();
    let start = Instant::now();
    while !src.hub.stack(&key).is_some_and(|s| s.link.is_live()) {
        assert!(start.elapsed() < Duration::from_secs(10), "no stack");
        std::thread::sleep(Duration::from_millis(20));
    }
    let (hub, first, facts) = (src.hub.clone(), src.first.clone(), src.facts.clone());
    let (root, vcx) = cx.add_window_view(|window, cx| Root::new(hub, first, facts, window, cx));
    let start = Instant::now();
    loop {
        vcx.update(|_, cx| {
            root.update(cx, |r: &mut Root, cx| {
                r.refresh(cx);
                r.overview.update(cx, |o, cx| o.set_view(View::Map, cx));
            })
        });
        vcx.run_until_parked();
        let n = vcx.update(|_, cx| root.read(cx).map.read(cx).layout.cards.len());
        if n > 60 || start.elapsed() > Duration::from_secs(10) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let m = vcx.update(|_, cx| root.read(cx).map.clone());
    vcx.update(|window, cx| {
        m.update(cx, |m, cx| {
            m.cam.size = (1440.0, 800.0);
            m.refit();
            window.focus(&m.focus, cx);
        })
    });
    (src, m, vcx)
}

fn name_of(m: &Entity<MapView>, vcx: &mut VisualTestContext) -> Option<String> {
    vcx.update(|_, cx| {
        let m = m.read(cx);
        m.lit.and_then(|c| m.layout.card(c)).map(|c| c.name.clone())
    })
}

/// Find: the search box's Enter selects the card and flies there, eased, ending at 1:1 on it; with motion reduced the
/// camera is simply there.
#[gpui::test]
fn find_flies_to_the_card(cx: &mut TestAppContext) {
    let (_src, m, vcx) = map(cx, "medium-150");
    let before = vcx.update(|_, cx| m.read(cx).cam);
    assert!(before.zoom < 0.5, "fitted: {}", before.zoom);
    vcx.update(|window, cx| m.update(cx, |m, cx| m.open_search(window, cx)));
    vcx.run_until_parked();
    vcx.simulate_input("search-api");
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(name_of(&m, vcx).as_deref(), Some("search-api"));
    let (flying, target) = vcx.update(|_, cx| {
        let m = m.read(cx);
        let id = m.lit.unwrap();
        (
            m.cam.flight.is_some(),
            m.layout.card(id).unwrap().rect.center(),
        )
    });
    assert!(flying, "a flight, not a jump");
    std::thread::sleep(Duration::from_millis(320));
    let cam = vcx.update(|_, cx| {
        m.update(cx, |m, _| {
            m.cam.tick(Instant::now());
            m.cam
        })
    });
    assert!(cam.flight.is_none(), "landed");
    assert!((cam.zoom - 1.0).abs() < 1e-3, "{}", cam.zoom);
    // Centred in what the detail card (top right) leaves of the viewport.
    let (sx, sy) = cam.to_screen(target.x, target.y);
    let (vw, vh) = cam.size;
    let free = vw - super::detail::WIDTH - 24.0;
    assert!((sx - free / 2.0).abs() < 2.0, "{sx} of {vw}");
    assert!((sy - vh / 2.0).abs() < 2.0, "{sy} of {vh}");
    // The breadcrumb's region: search.
    let region = vcx.update(|_, cx| {
        let m = m.read(cx);
        m.layout.region(m.lit.unwrap()).map(|r| r.name.clone())
    });
    assert_eq!(region.as_deref(), Some("search"));
    // Reduced motion: there at once.
    vcx.update(|_, cx| cx.set_reduce_motion(true));
    vcx.update(|_, cx| {
        m.update(cx, |m, cx| {
            m.refit();
            let id = m.layout.card_named("payments-api").unwrap();
            m.jump_to(id, cx);
            assert!(m.cam.flight.is_none());
            assert!((m.cam.zoom - 1.0).abs() < 1e-3);
        })
    });
}

/// H on a selected card: its neighbourhood at two hops; H again, three; on another card, centred there; esc back to
/// the whole map and the camera it had.
#[gpui::test]
fn h_opens_a_neighbourhood_and_esc_comes_back(cx: &mut TestAppContext) {
    let (_src, m, vcx) = map(cx, "medium-150");
    let (whole, cam) = vcx.update(|_, cx| {
        m.update(cx, |m, cx| {
            let id = m.layout.card_named("checkout-api");
            m.select(id, cx);
            (m.layout.cards.len(), m.cam)
        })
    });
    vcx.simulate_keystrokes("shift-h");
    vcx.run_until_parked();
    let (lens, mode, n) = vcx.update(|_, cx| {
        let m = m.read(cx);
        (m.lens.clone(), m.layout.mode, m.layout.cards.len())
    });
    assert_eq!(
        lens,
        Some(Lens::Hood {
            centre: "checkout-api".into(),
            hops: 2
        })
    );
    assert_eq!(mode, doze_map::Mode::Focus);
    assert!(n > 3 && n < whole, "{n} of {whole}");
    vcx.simulate_keystrokes("shift-h");
    vcx.run_until_parked();
    let n3 = vcx.update(|_, cx| m.read(cx).layout.cards.len());
    assert!(matches!(
        vcx.update(|_, cx| m.read(cx).lens.clone()),
        Some(Lens::Hood { hops: 3, .. })
    ));
    assert!(n3 >= n);
    // Another card, then H: centred there, the hops kept.
    vcx.update(|_, cx| {
        m.update(cx, |m, cx| {
            let id = m.layout.card_named("checkout-cart");
            m.select(id, cx);
        })
    });
    vcx.simulate_keystrokes("shift-h");
    vcx.run_until_parked();
    assert_eq!(
        vcx.update(|_, cx| m.read(cx).lens.clone()),
        Some(Lens::Hood {
            centre: "checkout-cart".into(),
            hops: 3
        })
    );
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    let (lens, n, back) = vcx.update(|_, cx| {
        let m = m.read(cx);
        (m.lens.clone(), m.layout.cards.len(), m.cam)
    });
    assert_eq!(lens, None);
    assert_eq!(n, whole);
    assert!((back.zoom - cam.zoom).abs() < 1e-4 && back.pan == cam.pan);
}

/// L past sixty cards: every line, and back; T: only trouble, and back.
#[gpui::test]
fn l_draws_every_line_and_t_shows_only_trouble(cx: &mut TestAppContext) {
    let (_src, m, vcx) = map(cx, "medium-150");
    assert!(vcx.update(|_, cx| m.read(cx).quiet()));
    vcx.simulate_keystrokes("shift-l");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| m.read(cx).all_lines && !m.read(cx).quiet()));
    vcx.simulate_keystrokes("shift-l");
    vcx.run_until_parked();
    assert!(vcx.update(|_, cx| m.read(cx).quiet()));
    vcx.simulate_keystrokes("shift-t");
    vcx.run_until_parked();
    let lens = vcx.update(|_, cx| m.read(cx).lens.clone());
    let Some(Lens::Trouble { troubled, .. }) = lens else {
        panic!("{lens:?}")
    };
    assert!(troubled >= 3, "{troubled}");
    let all_bad = vcx.update(|_, cx| {
        let m = m.read(cx);
        m.layout
            .cards
            .iter()
            .filter(|c| m.rows.get(&c.name).is_some_and(|r| r.trouble))
            .count()
    });
    assert_eq!(all_bad, troubled);
    vcx.simulate_keystrokes("shift-t");
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_, cx| m.read(cx).lens.clone()), None);
}
