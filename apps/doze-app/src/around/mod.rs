//! Around the clock (M3 steps 3.1 and 3.2): what the app does whether or not its window is open. The menu-bar item
//! (`crate::tray`) and the notifications both read one Needs you list: the window's while it is open (the bell's own,
//! so the two never disagree), and while it is closed one built here from the hub, the same way and kept steady by
//! the same rules. Notifications follow Go's D8 rules (`doze_desk::notify`): through the system's notification
//! centre when the app runs from its bundle, else said in the app (a toast, kept among Recent) and on stderr.
//!
//! - [`start`] puts the item in the menu bar, hears its picks and the notifications' clicks, and hears acting while
//!   no window is open: a question or a sheet raised from the menu bar opens the window to ask it;
//! - [`on_batch`] is the hub moving: notes, the list (while closed), the menu;
//! - [`window_list`] is the window's list, each time it builds one.

pub(crate) mod notes;
mod picks;
#[cfg(all(test, feature = "capture"))]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, SystemTime};

use doze_desk::Hub;
use doze_desk::attention::{Attention, Item, Steady};
use doze_desk::notify::{Native, Note, Rules};
use gpui::{App, AsyncApp, Global};

use crate::act::{Acts, ActsEvent};
use crate::tray::Menu;

/// How long an item waiting out its grace or its "fixed" waits before the list is built again (as the window's).
const LOOK_AGAIN: Duration = Duration::from_secs(3);

/// The app's round-the-clock state.
pub(crate) struct Around {
    hub: Hub,
    menu: Option<Menu>,
    /// Needs you while no window is open.
    steady: Steady,
    /// The list the menu and the notifications read.
    pub(crate) list: Attention,
    /// The machine's items from the window's last list: doctor runs only while a window shows, so these are carried.
    machine: Vec<Item>,
    building: bool,
    dirty: bool,
    timer: bool,
    rules: Rules,
    /// What the rules said, waiting a moment for Needs you to list its item (so its button can be offered).
    waiting: Vec<(Note, std::time::Instant)>,
    flushing: bool,
    /// The notifications posted, oldest first, by tag: what a click on one acts on.
    posted: Vec<(String, (Note, Native))>,
    /// Running from Doze.app: the system's notifications can be used.
    bundled: bool,
    /// What was put aside with Not now (read again each time the window builds its list, where it is changed).
    not_now: std::collections::BTreeMap<String, String>,
    /// The Dock badge as last set.
    badge: Option<usize>,
    /// When the menu bar and the rules last looked while the window was open, and whether a look is due at the end of
    /// the second.
    looked: Option<std::time::Instant>,
    later: bool,
    /// The names registry as last read, and the file's time then.
    names: (Option<SystemTime>, doze_desk::library::Names),
    /// Until when acting's questions came from outside the window (the menu bar, a notification), so they bring the
    /// window forward; one the window raised itself does not take the focus.
    pub(crate) outside_until: Option<std::time::Instant>,
    /// The newest of acting's toasts already looked at (`notes::failures`).
    said_up_to: u64,
}

impl Global for Around {}

/// The app runs from inside a `.app`, where the notification centre will take it (Go `inBundle`).
pub(crate) fn bundled() -> bool {
    std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
}

/// Puts the menu-bar item up and starts listening: its picks, the notifications' clicks, acting while closed.
pub(crate) fn start(cx: &mut App, hub: &Hub, mode: &crate::facts::Mode) {
    let acts = crate::act::ensure(hub, mode, cx);
    cx.subscribe(&acts, |acts, ev: &ActsEvent, cx| asked(acts, ev, cx))
        .detach();
    cx.observe(&acts, |acts, cx| notes::failures(&acts, cx))
        .detach();
    let menu = Menu::install(&hub.stacks(), hub.now()).map(|(m, picks)| {
        cx.spawn(async move |cx: &mut AsyncApp| {
            while let Ok(id) = picks.recv().await {
                cx.update(|cx| picks::picked(cx, id));
            }
        })
        .detach();
        m
    });
    if menu.is_none() {
        eprintln!("doze-app: no menu-bar item on this system; the window stays the way in");
    }
    let bundled = bundled();
    if bundled {
        cx.on_system_notification_response(notes::respond);
    }
    cx.set_global(Around {
        hub: hub.clone(),
        menu,
        steady: Steady::default(),
        list: Attention::default(),
        machine: Vec::new(),
        building: false,
        dirty: false,
        timer: false,
        rules: Rules::new(),
        waiting: Vec::new(),
        flushing: false,
        posted: Vec::new(),
        bundled,
        not_now: crate::prefs::recall::not_now(),
        badge: None,
        looked: None,
        later: false,
        names: (None, doze_desk::library::Names::default()),
        outside_until: None,
        said_up_to: 0,
    });
    on_batch(cx);
    open_later(cx);
}

/// `$DOZE_APP_TRAY_OPEN=SECONDS` opens the menu that long after launch, for a picture of the real menu
/// (`screencapture -l` of this process's menu window); a menu-bar item's menu cannot be rendered offscreen.
fn open_later(cx: &mut App) {
    let Some(secs) = std::env::var("DOZE_APP_TRAY_OPEN")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return;
    };
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor()
            .timer(Duration::from_secs(secs))
            .await;
        cx.update(|cx| {
            if let Some(m) = around(cx).and_then(|a| a.menu.as_ref()) {
                m.open();
            }
        });
    })
    .detach();
}

/// Acting raised a question, a sheet or a folder to show: with no window open it came from the menu bar or a
/// notification, so the window opens on the stack and asks there. With a window open the window hears it itself;
/// it is brought forward.
fn asked(acts: gpui::Entity<Acts>, ev: &ActsEvent, cx: &mut App) {
    let had = cx
        .try_global::<crate::app::State>()
        .and_then(|s| s.window)
        .is_some();
    let key = match ev {
        ActsEvent::Ask(a) => a.key.clone(),
        ActsEvent::Check(c) => Some(c.key.clone()),
        ActsEvent::Start(k, _) => Some(k.clone()),
        ActsEvent::Open(_) => None,
    };
    let outside = around(cx)
        .and_then(|a| a.outside_until.take())
        .is_some_and(|t| std::time::Instant::now() < t);
    if had && !outside {
        // The window raised it and hears it itself; the focus stays where the person left it.
        return;
    }
    let Some(h) = crate::app::open(cx) else {
        return;
    };
    if had {
        return;
    }
    if let Some(k) = key {
        let _ = h.update(cx, |root, window, cx| root.show_key(k, window, cx));
    }
    // The window is new: its root hears acting from now on, so it is told again.
    let ev = ev.clone();
    acts.update(cx, |_, cx| cx.emit(ev));
}

/// The hub moved: what to say outside, the list while no window builds it, and the menu.
pub(crate) fn on_batch(cx: &mut App) {
    if !cx.has_global::<Around>() {
        return;
    }
    // While the window is open the hub sends a batch a frame; the menu bar and the notification rules want a look a
    // second (as while it is closed), so a batch inside the second is folded into one look at its end.
    if window_open(cx) {
        let Some(a) = around(cx) else { return };
        let due = a.looked.is_none_or(|t| t.elapsed() >= EVERY);
        if !due {
            if !a.later {
                a.later = true;
                cx.spawn(async move |cx: &mut AsyncApp| {
                    cx.background_executor().timer(EVERY).await;
                    cx.update(|cx| {
                        if let Some(a) = around(cx) {
                            a.later = false;
                        }
                        on_batch(cx);
                    });
                })
                .detach();
            }
            return;
        }
        a.looked = Some(std::time::Instant::now());
        notes::look(cx);
        follow(cx);
    } else {
        notes::look(cx);
        rebuild(cx);
    }
}

/// How often the menu bar and the rules look while the window is open (the hub's pace while it is closed).
const EVERY: Duration = Duration::from_secs(1);

/// The window built its list: the menu and the notifications read it. The window's raw items step this list's own
/// steadiness too, so when the window closes the menu bar goes on from where the bell was (not from before the window
/// opened: an item fixed meanwhile would come back red for its grace, and a fixed task of a stopped stack for good).
pub(crate) fn window_list(
    list: &Attention,
    items: Vec<Item>,
    stacks: &[doze_desk::StackView],
    busy: &BTreeSet<String>,
    now: SystemTime,
    cx: &mut App,
) {
    let Some(a) = around(cx) else {
        return;
    };
    // What the window put aside was saved before this list was built.
    a.not_now = crate::prefs::recall::not_now();
    let _ = a
        .steady
        .step(items, stacks, &|k| busy.contains(k), &a.not_now, now);
    if a.list == *list {
        return;
    }
    a.list = list.clone();
    a.machine = list
        .items
        .iter()
        .filter(|i| i.key.is_empty() && !i.fixed)
        .cloned()
        .collect();
    follow(cx);
}

pub(crate) fn window_open(cx: &App) -> bool {
    cx.try_global::<crate::app::State>()
        .and_then(|s| s.window)
        .is_some()
}

/// The menu (rebuilt only when it would say something else) and the Dock's badge, from the list as it is.
fn follow(cx: &mut App) {
    let Some(a) = around(cx) else {
        return;
    };
    let (stacks, now) = (a.hub.stacks(), a.hub.now());
    let red = a.list.red;
    if let Some(m) = a.menu.as_mut() {
        // The names registry's URLs for "Open <url>" (port-less while the front door is up), read again only when the
        // file changed.
        let file = a.hub.home().join("names.json");
        let changed = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        if a.names.0 != changed || a.names.0.is_none() {
            a.names = (changed, doze_desk::library::Names::read(a.hub.home()));
        }
        let names = &a.names.1;
        m.follow(&stacks, &a.list, now, &|h| names.url_for(h));
    }
    if a.badge != Some(red) {
        a.badge = Some(red);
        doze_os::ui::dock_badge((red > 0).then(|| red.to_string()).as_deref());
    }
    if let Some(m) = a.menu.as_mut() {
        m.evidence(red);
    }
    notes::flush(cx);
}

/// Builds Needs you while no window does (off the UI thread: a crash's port is looked up with lsof).
fn rebuild(cx: &mut App) {
    let Some(a) = around(cx) else {
        return;
    };
    if a.building {
        a.dirty = true;
        return;
    }
    a.building = true;
    let (hub, machine) = (a.hub.clone(), a.machine.clone());
    let (busy, failed) = acting(cx);
    let job = cx.background_executor().spawn(async move {
        let (stacks, now) = (hub.stacks(), hub.now());
        let mut items = crate::shell::attn::items_now(&stacks, &failed, &[], now);
        items.extend(machine);
        (items, stacks, now)
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let (items, stacks, now) = job.await;
        cx.update(|cx| took(cx, items, &stacks, &busy, now));
    })
    .detach();
}

fn took(
    cx: &mut App,
    items: Vec<Item>,
    stacks: &[doze_desk::StackView],
    busy: &BTreeSet<String>,
    now: SystemTime,
) {
    let open = window_open(cx);
    let Some(a) = around(cx) else {
        return;
    };
    a.building = false;
    let list = a
        .steady
        .step(items, stacks, &|k| busy.contains(k), &a.not_now, now);
    if !open {
        a.list = list;
    }
    let (dirty, wait) = (a.dirty, a.steady.waiting() && !a.timer);
    a.dirty = false;
    follow(cx);
    if dirty {
        rebuild(cx);
    } else if wait {
        if let Some(a) = around(cx) {
            a.timer = true;
        }
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor().timer(LOOK_AGAIN).await;
            cx.update(|cx| {
                if let Some(a) = around(cx) {
                    a.timer = false;
                }
                on_batch(cx);
            });
        })
        .detach();
    }
}

/// What acting knows now: the stacks mid-action, and the failed actions, by stack folder (Needs you's keys).
fn acting(
    cx: &App,
) -> (
    BTreeSet<String>,
    HashMap<String, doze_desk::attention::StackFailure>,
) {
    let Some(acts) = crate::act::acts(cx) else {
        return Default::default();
    };
    let a = acts.read(cx);
    let dir = |k: &doze_desk::StackKey| k.dir().to_string_lossy().into_owned();
    (
        a.busy.keys().map(dir).collect(),
        a.failed.iter().map(|(k, f)| (dir(k), f.clone())).collect(),
    )
}

/// The app's [`Around`], once [`start`] made it.
pub(crate) fn around(cx: &mut App) -> Option<&mut Around> {
    if cx.has_global::<Around>() {
        Some(cx.global_mut::<Around>())
    } else {
        None
    }
}

/// Items of the stack `key` leave the menu bar's list without "fixed": the person dismissed or removed them (the
/// window's list is quieted beside it, so the two agree).
pub(crate) fn quiet_menu(key: &str, cx: &mut App) {
    if let Some(a) = around(cx) {
        a.steady.quiet(|i| i.key == key);
    }
}
