//! `doze-app shot`: a screen rendered offscreen to a PNG through GPUI's own renderer (`render_to_image`, behind the
//! `capture` feature): no screen-recording permission and no picture of the desktop. The window is fed by a fixture
//! daemon through the hub, the same path a live window takes; the shot is taken once the page has its stack, the Map
//! has fitted, and the entrance motion is over.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui::{App, AppContext as _, AsyncApp, Bounds, QuitMode, WindowHandle, px, size};

use crate::cli::{Screen, Shot};
use crate::shell::Root;
use crate::theme::Choice;

pub(crate) fn run(s: Shot) -> ExitCode {
    if s.from.fixture.is_none() && s.from.synth.is_none() {
        eprintln!("doze-app shot: name a stack with --fixture NAME or --synth N");
        return ExitCode::from(2);
    }
    let source = match crate::source_for(&s.from, std::env::temp_dir()) {
        Ok(src) => std::rc::Rc::new(src),
        Err(e) => {
            eprintln!("doze-app shot: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx: &mut App| {
            crate::app::boot_core(cx);
            crate::app::boot_ui(
                cx,
                Choice {
                    palette: s.palette,
                    mode: s.mode,
                },
            );
            if s.reduce {
                cx.set_reduce_motion(true);
            }
            let hub = source.hub.clone();
            hub.set_visible(true);
            let bounds = Bounds::centered(None, size(px(s.size.0), px(s.size.1)), cx);
            let first = source.first.clone();
            let Ok(h) = cx.open_window(crate::app::options(bounds, true), |window, cx| {
                cx.new(|cx| Root::new(hub.clone(), first, window, cx))
            }) else {
                eprintln!("doze-app shot: no window");
                cx.quit();
                return;
            };
            let keep = source.clone();
            cx.spawn(async move |cx: &mut AsyncApp| {
                let _keep = keep;
                take(cx, h, &s).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    ExitCode::SUCCESS
}

async fn pause(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

/// Waits for the stack and the first fit, sets the scene up, renders, saves.
async fn take(cx: &mut AsyncApp, h: WindowHandle<Root>, s: &Shot) {
    let start = Instant::now();
    loop {
        let ready = h.update(cx, |root, _, cx| {
            root.refresh(cx);
            let m = root.map.read(cx);
            root.key().is_some() && m.cam.fitted && !m.layout.cards.is_empty()
        });
        if ready.unwrap_or(false) || start.elapsed() > Duration::from_secs(10) {
            break;
        }
        let _ = h.update(cx, |_, w, _| w.refresh());
        pause(cx, 50).await;
    }
    let _ = h.update(cx, |root, window, cx| stage(root, window, cx, s));
    pause(cx, 300).await;
    if let Some(q) = &s.find {
        let _ = h.update(cx, |root, window, cx| {
            root.map.update(cx, |m, cx| {
                m.open_search(window, cx);
                if let Some(input) = m.search.clone() {
                    input.update(cx, |i, cx| i.set_value(q.clone(), window, cx));
                }
                m.hits = m.layout.search(q).into_iter().take(8).collect();
            });
        });
    }
    // Let the entrance motion finish (280 ms at most) and the cache refine.
    pause(cx, 700).await;
    let _ = h.update(cx, |_, w, _| w.refresh());
    pause(cx, 400).await;
    match h.update(cx, |_, window, _| window.render_to_image()) {
        Ok(Ok(img)) => match img.save(&s.out) {
            Ok(()) => println!("{} {}x{}", s.out.display(), img.width(), img.height()),
            Err(e) => eprintln!("doze-app shot: save: {e}"),
        },
        Ok(Err(e)) => eprintln!("doze-app shot: render: {e:#}"),
        Err(e) => eprintln!("doze-app shot: window: {e:#}"),
    }
}

/// The camera, the selection, the hover and the overlay the shot asks for.
fn stage(root: &mut Root, window: &mut gpui::Window, cx: &mut gpui::Context<Root>, s: &Shot) {
    root.map.update(cx, |m, cx| {
        if let Some(z) = s.zoom {
            let f = z / m.cam.zoom;
            m.cam.zoom_centre(f, m.layout.bounds);
        }
        if let Some((x, y)) = s.at {
            m.cam.centre_on(x, y, m.layout.bounds);
        }
        if let Some(id) = s.select.as_deref().and_then(|n| m.card_named(n)) {
            m.lit = Some(id);
            if s.zoom.is_some() && s.at.is_none() {
                let c = m.layout.card(id).map(|c| c.rect.center());
                if let Some(c) = c {
                    m.cam.centre_on(c.x, c.y, m.layout.bounds);
                }
            }
        }
        m.hover = s.hover.as_deref().and_then(|n| m.card_named(n));
        cx.notify();
    });
    match s.screen {
        Screen::Map => {}
        Screen::Palette => root.on_palette(&crate::actions::OpenPalette, window, cx),
        Screen::Help => root.on_help(&crate::actions::ShowHelp, window, cx),
    }
}
