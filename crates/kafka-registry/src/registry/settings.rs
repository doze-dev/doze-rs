//! Compatibility levels and modes, of a subject and of the whole registry (the empty subject). A subject without its own
//! has the global one; the global level is BACKWARD and the global mode READWRITE until set.

use super::{Registry, State, check_subject};
use crate::error::{RegistryError, code};
use crate::store::{Batch, Key, Setting};

/// A compatibility level, as Confluent names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    None,
    Backward,
    BackwardTransitive,
    Forward,
    ForwardTransitive,
    Full,
    FullTransitive,
}

impl Level {
    /// A level by its name, any case.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.to_ascii_uppercase().as_str() {
            "NONE" => Self::None,
            "BACKWARD" => Self::Backward,
            "BACKWARD_TRANSITIVE" => Self::BackwardTransitive,
            "FORWARD" => Self::Forward,
            "FORWARD_TRANSITIVE" => Self::ForwardTransitive,
            "FULL" => Self::Full,
            "FULL_TRANSITIVE" => Self::FullTransitive,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Backward => "BACKWARD",
            Self::BackwardTransitive => "BACKWARD_TRANSITIVE",
            Self::Forward => "FORWARD",
            Self::ForwardTransitive => "FORWARD_TRANSITIVE",
            Self::Full => "FULL",
            Self::FullTransitive => "FULL_TRANSITIVE",
        }
    }

    /// Whether the new schema must read data written with the old.
    #[must_use]
    pub fn backward(self) -> bool {
        matches!(
            self,
            Self::Backward | Self::BackwardTransitive | Self::Full | Self::FullTransitive
        )
    }

    /// Whether the old schema must read data written with the new.
    #[must_use]
    pub fn forward(self) -> bool {
        matches!(
            self,
            Self::Forward | Self::ForwardTransitive | Self::Full | Self::FullTransitive
        )
    }

    /// Whether every live version is compared, not only the latest.
    #[must_use]
    pub fn transitive(self) -> bool {
        matches!(
            self,
            Self::BackwardTransitive | Self::ForwardTransitive | Self::FullTransitive
        )
    }
}

/// A mode. IMPORT and READONLY_OVERRIDE are not offered (docs/port/behaviours/registry.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadWrite,
    ReadOnly,
}

impl Mode {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "READWRITE" => Some(Self::ReadWrite),
            "READONLY" => Some(Self::ReadOnly),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ReadWrite => "READWRITE",
            Self::ReadOnly => "READONLY",
        }
    }
}

impl State {
    /// The level that applies to a subject: its own, else the global one.
    pub(super) fn level_of(&self, subject: &str) -> Level {
        let own = (!subject.is_empty())
            .then(|| self.levels.get(subject))
            .flatten();
        own.or_else(|| self.levels.get(""))
            .and_then(|s| Level::parse(&s.value))
            .unwrap_or(Level::Backward)
    }

    pub(super) fn mode_of(&self, subject: &str) -> Mode {
        let own = (!subject.is_empty())
            .then(|| self.modes.get(subject))
            .flatten();
        own.or_else(|| self.modes.get(""))
            .and_then(|s| Mode::parse(&s.value))
            .unwrap_or(Mode::ReadWrite)
    }
}

impl Registry {
    /// The level of a subject (`""`: the global one). With `fallback`, a subject without its own reports the global one.
    ///
    /// # Errors
    /// The subject has no level of its own and `fallback` is off.
    pub fn level(&self, subject: &str, fallback: bool) -> Result<Level, RegistryError> {
        let s = self.lock();
        if subject.is_empty() || fallback || s.levels.contains_key(subject) {
            return Ok(s.level_of(subject));
        }
        Err(RegistryError::new(
            404,
            code::SUBJECT_COMPAT_NOT_SET,
            format!("Subject '{subject}' does not have subject-level compatibility configured"),
        ))
    }

    /// Sets the level of a subject (`""`: the global one).
    ///
    /// # Errors
    /// An invalid level or subject, a read-only subject, a store failure.
    pub fn set_level(&self, subject: &str, level: &str) -> Result<(), RegistryError> {
        let level = Level::parse(level).ok_or_else(RegistryError::invalid_compatibility)?;
        check_subject(subject)?;
        let mut s = self.lock();
        // A read-only subject's level does not change either (recorded: readonly-subject).
        if !subject.is_empty() && s.mode_of(subject) == Mode::ReadOnly {
            return Err(RegistryError::read_only(subject));
        }
        let setting = Setting {
            value: level.name().to_owned(),
            at: (self.clock)(),
        };
        let mut b = Batch::default();
        b.put(&Key::Level(subject.to_owned()), &setting);
        self.commit(&b)?;
        s.levels.insert(subject.to_owned(), setting);
        Ok(())
    }

    /// Removes a subject's own level (the global one goes back to the default) and returns what applied before.
    ///
    /// # Errors
    /// The subject has no level of its own; a store failure.
    pub fn delete_level(&self, subject: &str) -> Result<Level, RegistryError> {
        let mut s = self.lock();
        let before = s.level_of(subject);
        if !subject.is_empty() && !s.levels.contains_key(subject) {
            return Err(RegistryError::subject_not_found(subject));
        }
        let mut b = Batch::default();
        b.delete(&Key::Level(subject.to_owned()));
        self.commit(&b)?;
        s.levels.remove(subject);
        Ok(before)
    }

    /// The mode of a subject (`""`: the global one). With `fallback`, a subject without its own reports the global one.
    ///
    /// # Errors
    /// The subject has no mode of its own and `fallback` is off.
    pub fn mode(&self, subject: &str, fallback: bool) -> Result<Mode, RegistryError> {
        let s = self.lock();
        if subject.is_empty() || fallback || s.modes.contains_key(subject) {
            return Ok(s.mode_of(subject));
        }
        Err(mode_not_set(subject))
    }

    /// Sets the mode of a subject (`""`: the global one).
    ///
    /// # Errors
    /// An invalid mode or subject, a store failure.
    pub fn set_mode(&self, subject: &str, mode: &str) -> Result<Mode, RegistryError> {
        let mode = Mode::parse(mode).ok_or_else(RegistryError::invalid_mode)?;
        check_subject(subject)?;
        let mut s = self.lock();
        let setting = Setting {
            value: mode.name().to_owned(),
            at: (self.clock)(),
        };
        let mut b = Batch::default();
        b.put(&Key::Mode(subject.to_owned()), &setting);
        self.commit(&b)?;
        s.modes.insert(subject.to_owned(), setting);
        Ok(mode)
    }

    /// Removes a subject's own mode and returns what applied before.
    ///
    /// # Errors
    /// The subject has no mode of its own; a store failure.
    pub fn delete_mode(&self, subject: &str) -> Result<Mode, RegistryError> {
        let mut s = self.lock();
        let before = s.mode_of(subject);
        if !subject.is_empty() && !s.modes.contains_key(subject) {
            return Err(mode_not_set(subject));
        }
        let mut b = Batch::default();
        b.delete(&Key::Mode(subject.to_owned()));
        self.commit(&b)?;
        s.modes.remove(subject);
        Ok(before)
    }
}

fn mode_not_set(subject: &str) -> RegistryError {
    RegistryError::new(
        404,
        code::SUBJECT_MODE_NOT_SET,
        format!("Subject '{subject}' does not have subject-level mode configured"),
    )
}
