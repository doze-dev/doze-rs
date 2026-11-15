//! The help sheet's glossary: the words a person meets on the screens, what each means, and its tone.

use doze_display::Look;

/// The words a person meets, what each means, and its tone (Go's help.tsx glossary). A state's word is the words' own
/// (`doze_display::Look`), so the sheet cannot drift from the rows; a task's three words, which Go's sheet left out, too.
pub(super) const WORDS: [(&str, &str, &str); 15] = [
    (
        Look::Awake.word(),
        "ok",
        "running, and something used it lately",
    ),
    (
        Look::Dozing.word(),
        "warn",
        "running, quiet for a while; sleeps when its timer runs out",
    ),
    (
        Look::Waking.word(),
        "cool",
        "starting, because something asked for it",
    ),
    (
        Look::Asleep.word(),
        "dim",
        "not running, data kept; the next connection wakes it",
    ),
    (
        Look::Crashing.word(),
        "bad",
        "exits soon after starting; doze tries again, a few times",
    ),
    (
        Look::Failed.word(),
        "bad",
        "doze stopped trying; it waits for you",
    ),
    (
        Look::Off.word(),
        "dim",
        "turned off: nothing wakes it until you switch it on",
    ),
    (
        Look::Paused.word(),
        "dim",
        "enabled = false in doze.hcl; only the file brings it back",
    ),
    ("pinned", "bright", "never sleeps, whatever holds it"),
    (
        Look::Running.word(),
        "cool",
        "a task running now; it keeps nothing awake once it ends",
    ),
    (Look::Done.word(), "ok", "a task whose last run went well"),
    (
        Look::NotRun.word(),
        "dim",
        "a task that has not run yet; it runs when you say",
    ),
    (
        doze_desk::overview::Look::STOPPED.word,
        "dim",
        "the whole stack is down; nothing runs",
    ),
    (
        "the switch",
        "bright",
        "on while it is up or on its way; flip it to wake or sleep",
    ),
    (
        "calls",
        "bright",
        "a reference in doze.hcl from one app to another",
    ),
];
