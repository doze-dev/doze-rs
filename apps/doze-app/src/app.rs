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
use crate::link::{self, Link};
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
    cx.set_menus(actions::menus());
    cx.on_action(|_: &Quit, cx| cx.quit());
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

/// The frame of a first open: the main screen's work area.
fn first_bounds(cx: &App) -> Bounds<gpui::Pixels> {
    match prefs::frame() {
        Some(f) => Bounds {
            origin: point(px(f.x as f32), px(f.y as f32)),
            size: size(px(f.w as f32), px(f.h as f32)),
        },
        None => cx.primary_display().map_or_else(
            || Bounds::centered(None, size(px(1280.), px(800.)), cx),
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
        focus: show,
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
        let _ = h.update(cx, |_, w, _| w.activate_window());
        cx.activate(true);
        return Some(h);
    }
    boot_ui(cx, prefs::theme());
    cx.set_activation_policy(ActivationPolicy::Regular);
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
    hub.set_visible(true);
    cx.global_mut::<State>().window = Some(h);
    cx.activate(true);
    Some(h)
}

/// Goes where a launch's arguments (or a `doze://` link) say: a stack, a service in it, a folder.
pub(crate) fn follow_args(cx: &mut App, args: &[String]) {
    let Some(h) = open(cx) else { return };
    let mut it = args.iter();
    let mut show: Option<String> = None;
    let mut target: Option<Result<Link, String>> = None;
    while let Some(a) = it.next() {
        if a == "--show" {
            show = it.next().cloned();
        } else if let Some(v) = a.strip_prefix("--show=") {
            show = Some(v.to_owned());
        } else if link::is(a) {
            target = Some(link::parse(a));
        } else {
            target = Some(Ok(Link::Open {
                dir: a.into(),
                show: None,
            }));
        }
    }
    let _ = h.update(cx, |root, window, cx| match target {
        Some(Ok(Link::Stack { stack, show: s })) => {
            if !root.show(&stack, s.or(show).as_deref(), window, cx) {
                root.say(format!("No stack called {stack} is known here."), cx);
            }
        }
        Some(Ok(Link::Open { dir, show: s })) => {
            root.open_dir(&dir, s.or(show).as_deref(), window, cx)
        }
        Some(Ok(Link::Library)) => root.go(crate::shell::Screen::Library, window, cx),
        Some(Ok(Link::Settings(part))) => {
            let part = part.unwrap_or_default();
            let at = crate::settings::Section::parse(&part);
            root.open_settings(at, part == "updates", window, cx);
        }
        Some(Err(e)) => root.say(e, cx),
        None => {
            if let Some(s) = show {
                root.select(&s, cx);
            }
        }
    });
}

/// A batch from the hub: the menu-bar item and the notifications always (`crate::around`); the window only while it
/// is open.
fn pump(cx: &mut App, hub: Hub) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(batch) = hub.updates().recv().await {
            cx.update(|cx| {
                crate::around::on_batch(cx);
                if let Some(h) = cx.try_global::<State>().and_then(|s| s.window) {
                    let _ = h.update(cx, |root, _, cx| root.apply(&batch, cx));
                }
            });
        }
    })
    .detach();
}

/// Runs the app until it quits.
pub(crate) fn run(a: cli::App) -> ExitCode {
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
        while let Ok(u) = urls.recv().await {
            cx.update(|cx| follow_args(cx, &u));
        }
    })
    .detach();
    if !a.menu_bar {
        follow_args(cx, &a.forward);
    }
    #[cfg(feature = "fixtures")]
    crate::driver::start(cx);
}
