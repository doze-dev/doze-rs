//! `doze-app shot`: a screen rendered offscreen to a PNG through GPUI's own renderer (`render_to_image`, behind the
//! `capture` feature): no screen-recording permission and no picture of the desktop. The window is fed by a fixture
//! daemon through the hub, the same path a live window takes; the shot is taken once the page has its stack, the Map
//! has fitted, and the entrance motion is over.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui::{App, AppContext as _, AsyncApp, Bounds, QuitMode, WindowHandle, px, size};

use crate::cli::{Screen, Shot};
use crate::overview::View;
use crate::shell::{Root, Screen as Page};
use crate::theme::{Choice, Density};

pub(crate) fn run(s: Shot) -> ExitCode {
    if s.from.fixture.is_none() && s.from.synth.is_none() {
        eprintln!("doze-app shot: name a stack with --fixture NAME or --synth N");
        return ExitCode::from(2);
    }
    // A fixture's clock starts at the moment the Go fixtures were recorded at, so ages read as the goldens do.
    let source = match crate::source_for(&s.from, std::env::temp_dir()) {
        Ok(src) => std::rc::Rc::new(src),
        Err(e) => {
            eprintln!("doze-app shot: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    // A port held for the shot: a port clash names this process as its holder.
    let _held = s
        .hold
        .and_then(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok());
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
            // Far off the screen, so the pointer is never over it: a shot has no hover in it.
            let bounds = Bounds {
                origin: gpui::point(px(-20_000.), px(-20_000.)),
                size: size(px(s.size.0), px(s.size.1)),
            };
            let first = source.first.clone();
            let facts = source.facts.clone();
            let Ok(h) = cx.open_window(crate::app::options(bounds, true), |window, cx| {
                cx.new(|cx| Root::new(hub.clone(), first, facts.clone(), window, cx))
            }) else {
                eprintln!("doze-app shot: no window");
                cx.quit();
                return;
            };
            let pumped = hub.clone();
            cx.spawn(async move |cx: &mut AsyncApp| {
                // The window hears the hub's batches, as the app's pump has it.
                while let Ok(batch) = pumped.updates().recv().await {
                    let _ = h.update(cx, |root, _, cx| root.apply(&batch, cx));
                }
            })
            .detach();
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
        let ready = h.update(cx, |root, window, cx| {
            root.refresh(cx);
            ready(root, window, cx, s)
        });
        if ready.unwrap_or(false) || start.elapsed() > Duration::from_secs(10) {
            break;
        }
        let _ = h.update(cx, |_, w, _| w.refresh());
        pause(cx, 50).await;
    }
    let _ = h.update(cx, |root, window, cx| stage(root, window, cx, s));
    pause(cx, 300).await;
    let _ = h.update(cx, |root, window, cx| map_stage(root, window, cx, s));
    if matches!(s.screen, Screen::Instance | Screen::Logs) {
        // The meters and the logs stream once a page watches them.
        pause(cx, 1500).await;
        let _ = h.update(cx, |root, window, cx| inside(root, window, cx, s));
        pause(cx, 600).await;
    }
    // Let the entrance motion finish (280 ms at most), the CLI's answers land, and the cache refine.
    pause(cx, 700).await;
    let _ = h.update(cx, |root, _, cx| root.refresh(cx));
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

/// The page the shot wants has what it shows.
fn ready(
    root: &mut Root,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<Root>,
    s: &Shot,
) -> bool {
    let stacks = root.library.read(cx).lib.stacks.len();
    // The Map, the palette and the help sheet are shot over the Map view, which must be shown to fit.
    let over_map = matches!(s.screen, Screen::Map | Screen::Palette | Screen::Help);
    if over_map && root.key().is_some() && root.overview.read(cx).view != View::Map {
        root.overview.update(cx, |o, cx| o.set_view(View::Map, cx));
    }
    match s.screen {
        Screen::Library => s.from.fixture.as_deref() == Some("none") || stacks > 1,
        Screen::Map | Screen::Palette | Screen::Help if s.stack.is_none() => {
            let m = root.map.read(cx);
            root.key().is_some() && m.cam.fitted && !m.layout.cards.is_empty()
        }
        _ => {
            if let Some(key) = target(root, cx, s) {
                if root.key() != Some(&key) {
                    root.go(Page::Stack(key), window, cx);
                }
                !root.overview.read(cx).home.groups.is_empty()
                    || !root
                        .overview
                        .read(cx)
                        .home
                        .stopped
                        .as_ref()
                        .is_none_or(|st| st.problem.is_empty())
            } else {
                false
            }
        }
    }
}

/// The stack the shot names, or the fixture's running one.
fn target(root: &Root, cx: &gpui::App, s: &Shot) -> Option<doze_desk::StackKey> {
    let lib = &root.library.read(cx).lib;
    let row = match &s.stack {
        Some(name) => lib.stacks.iter().find(|r| &r.name == name),
        None => lib.stacks.iter().find(|r| r.running),
    }?;
    Some(doze_desk::StackKey::new(row.key.clone()))
}

/// The page, its view, the selection, the density, the Grid's cell, the overlay the shot asks for.
fn stage(root: &mut Root, window: &mut gpui::Window, cx: &mut gpui::Context<Root>, s: &Shot) {
    if let Some(d) = s.density.as_deref().and_then(Density::parse) {
        root.library.update(cx, |l, cx| l.set_density(d, cx));
        root.overview.update(cx, |o, cx| o.set_density(d, cx));
    }
    let view = match s.screen {
        Screen::Tiles => Some(View::Tiles),
        Screen::List => Some(View::List),
        Screen::Grid => Some(View::Grid),
        Screen::Map => Some(View::Map),
        _ => None,
    };
    match (s.screen, view) {
        (Screen::Library, _) => root.go(Page::Library, window, cx),
        (_, Some(v)) => {
            root.overview.update(cx, |o, cx| {
                o.set_view(v, cx);
                if let Some(sel) = &s.select {
                    o.select(sel, cx);
                }
                if let (View::Grid, Some(cell)) = (v, &s.hover) {
                    o.cur = grid_cell(o, cell);
                }
                // `--at X,Y` scrolls the Grid there (the sticky head and service column stay).
                if let (View::Grid, Some((x, y))) = (v, s.at) {
                    (o.grid.x, o.grid.y) = (x as f32, y as f32);
                }
            });
        }
        _ => {}
    }
    match s.screen {
        Screen::Palette => root.on_palette(&crate::actions::OpenPalette, window, cx),
        Screen::Help => root.on_help(&crate::actions::ShowHelp, window, cx),
        Screen::Instance => {
            if let Some(name) = &s.select {
                root.open_instance(name, window, cx);
            }
        }
        Screen::Logs => root.open_logs(window, cx),
        _ => {}
    }
}

/// `row/column` in the Grid, as indices.
fn grid_cell(o: &crate::overview::OverviewView, cell: &str) -> Option<(usize, usize)> {
    let (r, c) = cell.split_once('/')?;
    let g = &o.home.grid;
    Some((
        g.rows.iter().position(|x| x.name == r)?,
        g.cols.iter().position(|x| x.name == c)?,
    ))
}

/// The Map's camera, selection and hover, and its find box.
fn map_stage(root: &mut Root, window: &mut gpui::Window, cx: &mut gpui::Context<Root>, s: &Shot) {
    if s.screen != Screen::Map && s.screen != Screen::Palette && s.screen != Screen::Help {
        return;
    }
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
    if let Some(q) = &s.find {
        root.map.update(cx, |m, cx| {
            m.open_search(window, cx);
            if let Some(input) = m.search.clone() {
                input.update(cx, |i, cx| i.set_value(q.clone(), window, cx));
            }
            m.hits = m.layout.search(q).into_iter().take(8).collect();
        });
    }
}

/// The page inside as the shot asks: its tab, then the logs' level, filter and find mode.
fn inside(root: &mut Root, window: &mut gpui::Window, cx: &mut gpui::Context<Root>, s: &Shot) {
    if let (Some(v), Some(tab)) = (root.inside_instance(), &s.tab) {
        v.update(cx, |v, cx| v.set_tab(tab, window, cx));
    }
    let Some(l) = root.inside_logs(cx) else {
        return;
    };
    l.update(cx, |l, cx| {
        l.stage(s.level, s.filter.as_deref(), s.find_mode, window, cx)
    });
}
