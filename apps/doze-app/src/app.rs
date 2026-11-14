//! The app's life: boot (fonts, the component library, the theme, the keymap and menus from the action table), the
//! menu-bar item, and the one window, made on demand and dropped on close (Go D11): while it is closed the app is a
//! menu-bar app (activation policy Accessory) and the hub's batches reach the menu-bar item alone. The first open fills
//! the screen's work area; later opens restore the frame the window closed with (prefs, shared with the Go app).

use std::borrow::Cow;
use std::process::ExitCode;
use std::rc::Rc;

use doze_desk::{Hub, StackKey};
use gpui::{
    ActivationPolicy, App, AppContext as _, AsyncApp, Bounds, Global, QuitMode, TitlebarOptions,
    WindowBounds, WindowHandle, WindowOptions, point, px, size,
};

use crate::actions::{self, OpenPalette, Quit};
use crate::cli;
pub(crate) use crate::link::follow_args;
use crate::prefs::{self, Frame};
use crate::shell::Root;
use crate::single::{self, Launch};
use crate::source::Source;
use crate::theme::{self, Choice};

/// What the running app holds.
pub(crate) struct State {
    pub(crate) hub: Hub,
    pub(crate) first: Option<StackKey>,
    /// Where stopped stacks' shapes and the machine's report come from.
    pub(crate) facts: crate::facts::Mode,
    pub(crate) window: Option<WindowHandle<Root>>,
    _source: Rc<Source>,
}

impl Global for State {}

/// The component library, the theme and the keymap are in place.
struct UiReady;

impl Global for UiReady {}

/// What a menu-bar app needs at once: the fonts, the menus, the app-wide actions.
pub(crate) fn boot_core(cx: &mut App) {
    let _ = cx.text_system().add_fonts(
        crate::assets::FONTS
            .iter()
            .map(|f| Cow::Borrowed(*f))
            .collect(),
    );
    crate::handsoff::guard_keys(cx);
    cx.set_menus(actions::menus());
    cx.on_action(|_: &Quit, cx| cx.quit());
    // About from the menu with no window open: the window first, on Settings › About (Go opened it for every item).
    cx.on_action(|_: &actions::AboutDoze, cx| {
        if let Some(h) = open(cx) {
            let _ = h.update(cx, |root, window, cx| {
                root.open_settings(Some(crate::settings::Section::About), false, window, cx);
            });
        }
    });
    cx.on_action(|_: &OpenPalette, cx| {
        if let Some(h) = open(cx) {
            let _ = h.update(cx, |root, window, cx| {
                root.on_palette(&OpenPalette, window, cx)
            });
        }
    });
}

/// What a window needs: the component library (about 140 ms, most of it gpui-component resolving the system's
/// fonts), the theme, and the keymap (bound after the library's own, so ours wins). Once.
pub(crate) fn boot_ui(cx: &mut App, choice: Choice) {
    if cx.has_global::<UiReady>() {
        return;
    }
    gpui_component::init(cx);
    if let Err(e) = theme::apply(cx, choice, cx.window_appearance()) {
        eprintln!("doze-app: theme: {e:#}");
    }
    let keys = actions::keymap(cx);
    cx.bind_keys(keys);
    cx.set_global(UiReady);
}

/// Frames the process's windows have committed to the screen so far (the renderer's count; 0 where it keeps none).
/// The GPU driver holds about 150 MB for as long as frames keep coming, so an idle window must add none.
pub(crate) fn frames_drawn() -> u64 {
    #[cfg(target_os = "macos")]
    return gpui_apple::metal_renderer::frames_drawn();
    #[cfg(not(target_os = "macos"))]
    0
}

/// The frame of a first open: the main screen's work area; with no screen known, Go's 960×640.
fn first_bounds(cx: &App) -> Bounds<gpui::Pixels> {
    match prefs::frame() {
        Some(f) => Bounds {
            origin: point(px(f.x as f32), px(f.y as f32)),
            size: size(px(f.w as f32), px(f.h as f32)),
        },
        None => cx.primary_display().map_or_else(
            || Bounds::centered(None, size(px(960.), px(640.)), cx),
            |d| d.visible_bounds(),
        ),
    }
}

/// The window's options: client-drawn title bar with the traffic lights inset, the narrowest a page still fits.
pub(crate) fn options(bounds: Bounds<gpui::Pixels>, show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("doze".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        window_min_size: Some(size(px(760.), px(400.))),
        show,
        // A hands-off run (tests, shots, the bench, a script) never takes the keyboard (`crate::handsoff`).
        focus: show && !crate::handsoff::on(),
        ..Default::default()
    }
}

/// Opens the window, or brings it forward.
pub(crate) fn open(cx: &mut App) -> Option<WindowHandle<Root>> {
    let (hub, first, existing, facts) = {
        let s = cx.try_global::<State>()?;
        (s.hub.clone(), s.first.clone(), s.window, s.facts.clone())
    };
    if let Some(h) = existing {
        if !crate::handsoff::on() {
            let _ = h.update(cx, |_, w, _| w.activate_window());
            cx.activate(true);
        }
        return Some(h);
    }
    boot_ui(cx, prefs::theme());
    if !crate::handsoff::on() {
        cx.set_activation_policy(ActivationPolicy::Regular);
    }
    let opts = options(first_bounds(cx), true);
    let h = cx
        .open_window(opts, |window, cx| {
            window.on_window_should_close(cx, |window, _| {
                let b = window.window_bounds().get_bounds();
                let n = |p: gpui::Pixels| f32::from(p).round() as i64;
                prefs::save_frame(Frame {
                    x: n(b.origin.x),
                    y: n(b.origin.y),
                    w: n(b.size.width),
                    h: n(b.size.height),
                });
                true
            });
            cx.new(|cx| Root::new(hub.clone(), first, facts, window, cx))
        })
        .ok()?;
    // The meters follow the window's use (`Root::sync_hub`); a hands-off run streams them while the window lives.
    if crate::handsoff::on() {
        hub.set_visible(true);
    }
    cx.global_mut::<State>().window = Some(h);
    if crate::handsoff::on() {
        crate::handsoff::after_open(h.into(), cx);
    } else {
        cx.activate(true);
    }
    Some(h)
}

/// A batch from the hub: the menu-bar item and the notifications always (`crate::around`); the window only while it
/// is open.
fn pump(cx: &mut App, hub: Hub) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(batch) = hub.updates().recv().await {
            cx.update(|cx| {
                crate::around::on_batch(cx);
                crate::activity::remember_batch(&hub, &batch, cx);
                if let Some(h) = cx.try_global::<State>().and_then(|s| s.window) {
                    let _ = h.update(cx, |root, _, cx| root.apply(&batch, cx));
                }
            });
        }
    })
    .detach();
}

/// Runs the app until it quits.
pub(crate) fn run(mut a: cli::App) -> ExitCode {
    crate::handsoff::from_env();
    // A hands-off run on the owner's home would hand its arguments to his running app, which would come forward.
    if crate::handsoff::on() && std::env::var_os("DOZE_HOME").is_none() {
        eprintln!("doze-app: a hands-off or scripted run needs its own DOZE_HOME");
        return ExitCode::FAILURE;
    }
    // A relative folder is the launching folder's, and a launch inside a project with no folder named opens it (Go
    // `dirFrom`): resolved here, before a second launch hands them to the first, which runs elsewhere.
    if let Ok(here) = std::env::current_dir() {
        a.forward = crate::open::resolved(&a.forward, &here);
    }
    let home = match doze_os::home::doze_home() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("doze-app: no doze home: {e}");
            return ExitCode::FAILURE;
        }
    };
    let first = match single::claim(&home, &a.forward) {
        Ok(Launch::Forwarded) => return ExitCode::SUCCESS,
        Ok(Launch::First(f)) => f,
        Err(e) => {
            eprintln!("doze-app: {e}");
            return ExitCode::FAILURE;
        }
    };
    prefs::set_home(&home);
    let source = match crate::source_for(&a.from, home) {
        Ok(s) => Rc::new(s),
        Err(e) => {
            eprintln!("doze-app: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let (url_tx, urls) = async_channel::unbounded::<Vec<String>>();
    let app = gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .with_quit_mode(QuitMode::Explicit);
    app.on_open_urls(move |u| {
        let _ = url_tx.try_send(u);
    });
    // The Dock icon clicked while no window is open.
    app.on_reopen(|cx| {
        open(cx);
    });
    app.run(move |cx: &mut App| started(cx, &a, source, first, urls));
    ExitCode::SUCCESS
}

fn started(
    cx: &mut App,
    a: &cli::App,
    source: Rc<Source>,
    first: single::First,
    urls: async_channel::Receiver<Vec<String>>,
) {
    boot_core(cx);
    cx.set_activation_policy(ActivationPolicy::Accessory);
    let hub = source.hub.clone();
    #[cfg(feature = "fixtures")]
    crate::fixture::crash_later(source.clone(), cx);
    cx.set_global(State {
        hub: hub.clone(),
        first: source.first.clone(),
        facts: source.facts.clone(),
        window: None,
        _source: source,
    });
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            if let Some(s) = cx.try_global::<State>() {
                s.hub.set_visible(false);
            }
            cx.global_mut::<State>().window = None;
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
    })
    .detach();
    let facts = cx.global::<State>().facts.clone();
    crate::around::start(cx, &hub, &facts);
    hub.set_visible(false);
    pump(cx, hub);
    // The menu bar item is up; the window's libraries load on the next turn of the loop, so the item shows first and
    // a click on it finds them ready.
    cx.spawn(async move |cx: &mut AsyncApp| cx.update(|cx| boot_ui(cx, prefs::theme())))
        .detach();
    let (args, launches) = (first.args.clone(), first);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let _keep = launches;
        while let Ok(a) = args.recv().await {
            cx.update(|cx| follow_args(cx, &a));
        }
    })
    .detach();
    cx.spawn(async move |cx: &mut AsyncApp| {
        // A doze:// link clicked anywhere on the system: followed a moment later, as Go's (the hub has to know the stacks).
        while let Ok(u) = urls.recv().await {
            cx.update(|cx| crate::link::follow_later(cx, u));
        }
    })
    .detach();
    if !a.menu_bar {
        if a.forward.iter().any(|x| crate::link::is(x)) {
            crate::link::follow_later(cx, a.forward.clone());
        } else {
            follow_args(cx, &a.forward);
        }
    }
    #[cfg(feature = "fixtures")]
    crate::driver::start(cx);
}
