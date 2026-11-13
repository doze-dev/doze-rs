//! Notifications (Go `app/notify.go`, D8): the rules say what is news; each note waits a moment for Needs you to list
//! its item, so the notification carries the item's first fix as a button (Go posted at once and often had none);
//! then it goes to the notification centre from the bundle, or is said in the app. A click on one opens its
//! instance's page; its button presses the fix, if the item still needs it and (for a port's holder) the same pid
//! still holds it.

use std::time::{Duration, Instant};

use doze_desk::StackKey;
use doze_desk::notify::{Native, Note, first_fix, native};
use gpui::{App, AsyncApp, SystemNotification, SystemNotificationAction};

use super::{around, picks};
use crate::shell::Screen;

/// How long a note waits for its item to reach Needs you before it goes without a button.
const WAIT_FOR_ITEM: Duration = Duration::from_secs(2);
/// How many posted notifications a click can still act on.
const POSTED_KEPT: usize = 50;
/// The button's id.
const FIX: &str = "fix";

/// What is on screen now, as the rules ask: a stack's page shows all of it, an instance's page shows that one (Go
/// `reportOutside`); nothing while no window is open.
fn on_screen(cx: &App) -> Option<Screen> {
    let h = cx.try_global::<crate::app::State>()?.window?;
    h.read(cx).ok().map(|r| r.screen.clone())
}

/// The rules look at the stacks; what they say waits for its item, then goes.
pub(super) fn look(cx: &mut App) {
    let screen = on_screen(cx);
    let visible = |key: &str, inst: &str| match &screen {
        Some(Screen::Stack(k)) => k.dir().to_string_lossy() == key,
        Some(Screen::Instance(k, n)) => k.dir().to_string_lossy() == key && n == inst,
        _ => false,
    };
    let Some(a) = around(cx) else { return };
    let (stacks, now) = (a.hub.stacks(), a.hub.now());
    let fresh = a.rules.next(&stacks, &visible, now);
    let start = Instant::now();
    a.waiting.extend(fresh.into_iter().map(|n| (n, start)));
    flush(cx);
}

/// Posts the notes whose item Needs you lists now, or that waited long enough; looks again later for the rest.
pub(super) fn flush(cx: &mut App) {
    let Some(a) = around(cx) else { return };
    let listed = |n: &Note| {
        let id = format!("instance:{}/{}", n.key, n.instance);
        a.list.items.iter().any(|i| i.id == id)
    };
    let (ready, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut a.waiting)
        .into_iter()
        .partition(|(n, at)| listed(n) || at.elapsed() >= WAIT_FOR_ITEM);
    a.waiting = wait;
    let again = !a.waiting.is_empty() && !a.flushing;
    a.flushing |= again;
    for (n, _) in ready {
        post(cx, n);
    }
    if again {
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor().timer(WAIT_FOR_ITEM).await;
            cx.update(|cx| {
                if let Some(a) = around(cx) {
                    a.flushing = false;
                }
                flush(cx);
            });
        })
        .detach();
    }
}

/// One note, out: the notification centre from the bundle; else a toast (kept among Recent) and stderr.
fn post(cx: &mut App, note: Note) {
    let Some(a) = around(cx) else { return };
    let n = native(&note, &a.list);
    let bundled = a.bundled;
    record(&n, bundled);
    if bundled {
        a.posted.retain(|(tag, _)| *tag != n.tag);
        a.posted.push((n.tag.clone(), (note, n.clone())));
        let over = a.posted.len().saturating_sub(POSTED_KEPT);
        a.posted.drain(..over);
        cx.show_system_notification(SystemNotification {
            tag: n.tag.into(),
            title: n.title.into(),
            body: n.body.into(),
            actions: n
                .action
                .map(|label| SystemNotificationAction {
                    id: FIX.into(),
                    label: label.into(),
                })
                .into_iter()
                .collect(),
        });
        return;
    }
    eprintln!(
        "doze-app: would notify: {} — {}",
        n.title,
        n.body.replace('\n', " · ")
    );
    if let Some(acts) = crate::act::acts(cx) {
        let text = format!("{} — {}", n.title, note.body);
        acts.update(cx, |a, cx| a.say(text, true, cx));
    }
}

/// What was posted, a line a note, to `$DOZE_APP_NOTIFY_LOG` when it is set (evidence: a notification is not a
/// picture the app can take).
fn record(n: &Native, bundled: bool) {
    let Some(path) = std::env::var_os("DOZE_APP_NOTIFY_LOG") else {
        return;
    };
    let line = format!(
        "{} | tag={} | title={} | body={} | action={}\n",
        if bundled { "native" } else { "in-app" },
        n.tag,
        n.title,
        n.body.replace('\n', " ⏎ "),
        n.action.as_deref().unwrap_or("-")
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// A notification was clicked: its button presses the fix when the item still needs that very fix; anything else
/// (and a fix that cannot be pressed from here) opens the instance's page.
pub(super) fn respond(r: gpui::SystemNotificationResponse, cx: &mut App) {
    record_click(&r);
    let Some(a) = around(cx) else { return };
    let Some((note, n)) = a
        .posted
        .iter()
        .find(|(t, _)| r.tag.as_ref() == t.as_str())
        .map(|(_, p)| p.clone())
    else {
        crate::app::open(cx);
        return;
    };
    if r.action_id.as_ref().is_some_and(|id| id == FIX)
        && let Some((it, offered)) = &n.fix
        && let Some((it, f)) = first_fix(&a.list, &it.id)
        && (f.kind != doze_desk::attention::FixKind::Holder || f.pid == offered.pid)
        && picks::press(cx, &it, &f)
    {
        return;
    }
    show_instance(cx, &note.key, &note.instance);
}

fn record_click(r: &gpui::SystemNotificationResponse) {
    if let Some(path) = std::env::var_os("DOZE_APP_NOTIFY_LOG") {
        let line = format!("click | tag={} | action={:?}\n", r.tag, r.action_id);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
    }
}

/// The window, on an instance's page (Go `OpenAt`).
pub(super) fn show_instance(cx: &mut App, key: &str, instance: &str) {
    let Some(h) = crate::app::open(cx) else {
        return;
    };
    let screen = if instance.is_empty() {
        Screen::Stack(StackKey::new(key))
    } else {
        Screen::Instance(StackKey::new(key), instance.to_owned())
    };
    let _ = h.update(cx, |root, window, cx| root.go(screen, window, cx));
}
