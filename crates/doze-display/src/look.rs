//! How an instance's state reads: the word, the glyph, and the tone they are drawn in.

use std::borrow::Cow;

use crate::model::{Holder, Instance, Kind, State};

/// The colour class a piece of text is drawn in. A surface maps each to its own palette: these are the board's
/// classes, not colours, so a theme can change what they look like and not what they mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Well.
    Good,
    /// Needs a look soon.
    Warn,
    /// On its way.
    Cool,
    /// Wrong.
    Bad,
    /// The rest.
    Dim,
    /// Ordinary text.
    Text,
    /// The brightest text: a name or a figure that is the point of its line.
    Bright,
}

impl Tone {
    /// The class's name, as the boards and the Go surfaces write it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Warn => "warn",
            Self::Cool => "cool",
            Self::Bad => "bad",
            Self::Dim => "dim",
            Self::Text => "text",
            Self::Bright => "bright",
        }
    }
}

/// How an instance's state reads. A task has its own looks (running, done, failed, not run) because it does not
/// sleep; the rest follow the daemon's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// Paused: `enabled = false` in doze.hcl.
    Paused,
    /// Off: taken out of this run.
    Off,
    /// A task running now.
    Running,
    /// Failed: a task's last run, or a start that did not complete.
    Failed,
    /// A task whose last run went well.
    Done,
    /// A task that has not run.
    NotRun,
    /// Up and held.
    Awake,
    /// Up with nobody holding it.
    Dozing,
    /// Starting.
    Waking,
    /// Exited by itself; being restarted.
    Crashing,
    /// Asleep.
    Asleep,
}

impl Look {
    /// Every look, in the order the help sheet explains them.
    pub const ALL: [Look; 11] = [
        Self::Awake,
        Self::Dozing,
        Self::Waking,
        Self::Asleep,
        Self::Crashing,
        Self::Failed,
        Self::Off,
        Self::Paused,
        Self::Running,
        Self::Done,
        Self::NotRun,
    ];

    /// The look whose word this is; `None` for any other word (a verb in flight, "stopped").
    #[must_use]
    pub fn of_word(word: &str) -> Option<Look> {
        Self::ALL.into_iter().find(|l| l.word() == word)
    }

    /// The word.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Off => "off",
            Self::Running => "running",
            Self::Failed => "failed",
            Self::Done => "done",
            Self::NotRun => "not run",
            Self::Awake => "awake",
            Self::Dozing => "dozing",
            Self::Waking => "waking",
            Self::Crashing => "crashing",
            Self::Asleep => "asleep",
        }
    }

    /// The glyph drawn before the word.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Paused => "⊘",
            Self::Off => "○",
            Self::Running | Self::Waking => "◌",
            Self::Failed | Self::Crashing => "✗",
            Self::Done => "✓",
            Self::NotRun => "▸",
            Self::Awake => "●",
            Self::Dozing => "◐",
            Self::Asleep => "·",
        }
    }

    /// The tone both are drawn in.
    #[must_use]
    pub fn tone(self) -> Tone {
        match self {
            Self::Paused | Self::Off | Self::NotRun | Self::Asleep => Tone::Dim,
            Self::Running | Self::Waking => Tone::Cool,
            Self::Failed | Self::Crashing => Tone::Bad,
            Self::Done | Self::Awake => Tone::Good,
            Self::Dozing => Tone::Warn,
        }
    }
}

/// The look of an instance. (Go: `LookOf`.)
#[must_use]
pub fn look(v: &Instance) -> Look {
    let booting = v.state == State::Booting;
    if v.paused {
        Look::Paused
    } else if v.disabled {
        Look::Off
    } else if v.is_task() && booting {
        Look::Running
    } else if let Some(task) = v.task.as_ref().filter(|_| !booting) {
        if task.failed() {
            Look::Failed
        } else {
            Look::Done
        }
    } else if v.is_task() {
        Look::NotRun
    } else {
        match v.state {
            State::Active => Look::Awake,
            State::Idle => Look::Dozing,
            State::Booting => Look::Waking,
            State::Crashing => Look::Crashing,
            State::Failed | State::Error => Look::Failed,
            State::Reaped | State::Other(_) => Look::Asleep,
        }
    }
}

/// The instance is up and serving: awake or dozing. Unlike [`State::awake`], which counts a process still booting,
/// this is the screens' sense of the word: a figure of memory, a column of connections. (Go: `AwakeNow`.)
#[must_use]
pub fn awake_now(v: &Instance) -> bool {
    matches!(v.state, State::Active | State::Idle)
}

/// Something is wrong with it: it is crashing, has failed, or is a task whose last run failed. (Go: `Troubled`.)
#[must_use]
pub fn troubled(v: &Instance) -> bool {
    matches!(v.state, State::Crashing | State::Failed | State::Error) || v.last_run_failed()
}

/// Where its switch stands, the same on every surface: on while it is up or on its way (awake, dozing, waking, and
/// crashing, which is doze still trying), off while it is asleep, has failed, is off or paused. A task has no
/// switch. (Go: `SwitchOn`.)
#[must_use]
pub fn switch_on(v: &Instance) -> bool {
    !(v.disabled || v.paused || v.kind == Kind::Task)
        && matches!(
            v.state,
            State::Active | State::Idle | State::Booting | State::Crashing
        )
}

/// The order a sort by state puts instances in: what needs you first, then waking, awake, dozing, and asleep last.
/// (Go: `StateRank`.)
#[must_use]
pub fn state_rank(v: &Instance) -> u8 {
    if troubled(v) {
        return 0;
    }
    match v.state {
        State::Booting => 1,
        State::Active => 2,
        State::Idle => 3,
        _ => 4,
    }
}

/// What is using it right now, in a few words ("api, 3 connection(s)"), or `None` when nothing is: the rule for
/// whether sleeping or restarting it asks first. (Go: `InUse`.)
#[must_use]
pub fn in_use(v: &Instance) -> Option<String> {
    let by: Vec<Cow<'_, str>> = v
        .holders
        .iter()
        .filter_map(|h| match h {
            Holder::Dependant(name) => Some(Cow::Borrowed(name.as_str())),
            Holder::Connections(n) => Some(Cow::Owned(format!("{n} connection(s)"))),
            Holder::Pin | Holder::Other => None,
        })
        .collect();
    let said = by.join(", ");
    (!said.is_empty()).then_some(said)
}
