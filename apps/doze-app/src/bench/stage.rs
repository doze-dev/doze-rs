//! `bench --screen`: the page the idle window is left on, so each page's own redraws are counted: Home (the Library),
//! a stack's page in one of its views, a service's page, All logs, Activity. The stack is the one shown, else the
//! first running one.

use gpui::{Context, Window};

use crate::overview::View;
use crate::shell::{Root, Screen as Page};

/// A page to leave the window on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Screen {
    /// Where the window opens (a fixture's stack, else the page it last had).
    #[default]
    Opened,
    Home,
    Tiles,
    List,
    Map,
    Grid,
    /// A service's page (`--select NAME`, else the stack's first service).
    Service,
    Logs,
    Activity,
}

impl Screen {
    pub(crate) fn parse(s: &str) -> Option<Screen> {
        Some(match s {
            "home" | "library" => Screen::Home,
            "tiles" => Screen::Tiles,
            "list" => Screen::List,
            "map" => Screen::Map,
            "grid" => Screen::Grid,
            "service" | "instance" => Screen::Service,
            "logs" => Screen::Logs,
            "activity" => Screen::Activity,
            _ => return None,
        })
    }

    fn view(self) -> Option<View> {
        Some(match self {
            Screen::Tiles => View::Tiles,
            Screen::List => View::List,
            Screen::Map => View::Map,
            Screen::Grid => View::Grid,
            _ => return None,
        })
    }
}

/// Leaves the window on `screen`.
pub(crate) fn go(
    root: &mut Root,
    hub: &doze_desk::Hub,
    screen: Screen,
    select: Option<&str>,
    window: &mut Window,
    cx: &mut Context<Root>,
) {
    if screen == Screen::Opened {
        return;
    }
    if screen == Screen::Home {
        root.go(Page::Library, window, cx);
        return;
    }
    let key = root.key().cloned().or_else(|| {
        let stacks = hub.stacks();
        stacks
            .iter()
            .find(|s| s.running)
            .or(stacks.first())
            .map(|s| s.key.clone())
    });
    let Some(key) = key else {
        eprintln!("doze-app bench: no stack to show");
        return;
    };
    root.go(Page::Stack(key.clone()), window, cx);
    if let Some(v) = screen.view() {
        root.overview.update(cx, |o, cx| o.set_view(v, cx));
        return;
    }
    match screen {
        Screen::Service => {
            let name = select.map(str::to_owned).or_else(|| {
                hub.stack(&key)
                    .and_then(|st| st.instances.first().map(|i| i.name.clone()))
            });
            if let Some(n) = name {
                root.open_instance(&n, window, cx);
            }
        }
        Screen::Logs => root.open_logs(window, cx),
        Screen::Activity => root.open_activity(window, cx),
        _ => {}
    }
}
