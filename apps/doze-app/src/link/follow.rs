//! Following a launch's arguments or a `doze://` link (Go `desk.follow`): the window opens where they point. A stack
//! named by its name is looked for among the hub's stacks for up to three seconds (15 × 200 ms, Go `stackDir`), as the
//! hub may not have looked yet just after launch; a link that points nowhere is said, and Home shows.

use std::time::Duration;

use gpui::{App, AsyncApp, WindowHandle};

use super::Link;
use crate::shell::Root;

/// How long a link waits after launch before it is followed (Go: `time.AfterFunc(300ms)`): the hub has to know the
/// stacks before a name can be found.
pub(crate) const AFTER_LAUNCH: Duration = Duration::from_millis(300);
/// How often, and how many times, a stack's name is looked for while the hub warms up (Go `stackDir`).
const LOOK_EVERY: Duration = Duration::from_millis(200);
const LOOKS: usize = 15;

/// What Go says of a link to a stack it does not know.
pub(crate) fn unknown(stack: &str) -> String {
    format!("no stack called {stack} here; Home lists the ones doze knows")
}

/// [`follow_args`] after [`AFTER_LAUNCH`], for a link handed over as the app starts.
pub(crate) fn follow_later(cx: &mut App, args: Vec<String>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(AFTER_LAUNCH).await;
        cx.update(|cx| follow_args(cx, &args));
    })
    .detach();
}

/// Goes where a launch's arguments (or a `doze://` link) say: a stack, a service in it, a folder.
pub(crate) fn follow_args(cx: &mut App, args: &[String]) {
    let Some(h) = crate::app::open(cx) else {
        return;
    };
    let mut it = args.iter();
    let mut show: Option<String> = None;
    let mut target: Option<Result<Link, String>> = None;
    while let Some(a) = it.next() {
        if a == "--show" {
            show = it.next().cloned();
        } else if let Some(v) = a.strip_prefix("--show=") {
            show = Some(v.to_owned());
        } else if super::is(a) {
            target = Some(super::parse(a));
        } else {
            target = Some(Ok(Link::Open {
                dir: a.into(),
                show: None,
            }));
        }
    }
    let _ = h.update(cx, |root, window, cx| match target {
        Some(Ok(Link::Stack { stack, show: s })) => {
            let show = s.or(show);
            if !root.show(&stack, show.as_deref(), window, cx) {
                look_again(h, stack, show, cx);
            }
        }
        Some(Ok(Link::Open { dir, show: s })) => {
            root.open_dir(&dir, s.or(show).as_deref(), window, cx);
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

/// Looks for the stack `stack` again every [`LOOK_EVERY`], [`LOOKS`] times; found, it shows; never found, Home shows
/// and the link is said to point nowhere, in red.
fn look_again(
    h: WindowHandle<Root>,
    stack: String,
    show: Option<String>,
    cx: &mut gpui::Context<Root>,
) {
    cx.spawn(async move |_, cx| {
        for _ in 0..LOOKS {
            cx.background_executor().timer(LOOK_EVERY).await;
            let found = h
                .update(cx, |root, window, cx| {
                    root.show(&stack, show.as_deref(), window, cx)
                })
                .unwrap_or(true);
            if found {
                return;
            }
        }
        let _ = h.update(cx, |root, window, cx| {
            root.go(crate::shell::Screen::Library, window, cx);
            if let Some(acts) = crate::act::acts(cx) {
                acts.update(cx, |a, cx| a.say(unknown(&stack), true, cx));
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::{AFTER_LAUNCH, LOOK_EVERY, LOOKS, unknown};

    /// Go's words and waits: three seconds of looking, 300 ms after launch.
    #[test]
    fn a_link_waits_as_go_s_did() {
        assert_eq!(
            unknown("shop"),
            "no stack called shop here; Home lists the ones doze knows"
        );
        assert_eq!(
            LOOK_EVERY * u32::try_from(LOOKS).unwrap(),
            std::time::Duration::from_secs(3)
        );
        assert_eq!(AFTER_LAUNCH.as_millis(), 300);
    }
}
