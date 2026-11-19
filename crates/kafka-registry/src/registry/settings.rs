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

/// A mode, as Confluent has them. IMPORT takes schemas with the ids (and versions) they are given and checks no
/// compatibility; READONLY_OVERRIDE is READONLY that, set globally, no subject's own mode overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadWrite,
    ReadOnly,
    ReadOnlyOverride,
    Import,
}

impl Mode {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "READWRITE" => Some(Self::ReadWrite),
            "READONLY" => Some(Self::ReadOnly),
            "READONLY_OVERRIDE" => Some(Self::ReadOnlyOverride),
            "IMPORT" => Some(Self::Import),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ReadWrite => "READWRITE",
            Self::ReadOnly => "READONLY",
            Self::ReadOnlyOverride => "READONLY_OVERRIDE",
            Self::Import => "IMPORT",
        }
    }

    /// Whether writes are refused.
    #[must_use]
    pub fn read_only(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadOnlyOverride)
    }
}

/// The settings Confluent keeps beside a level (`ConfigUpdateRequest`): each one set or not. `normalize` is applied (a
/// registration, lookup or check normalizes when its subject's, or else the global, setting says so); the others are kept
/// and answered as Confluent answers them but change nothing here (REG-D26). The metadata and rule sets are their JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConfigExtra {
    pub alias: Option<String>,
    pub normalize: Option<bool>,
    pub validate_fields: Option<bool>,
    pub compatibility_group: Option<String>,
    pub default_metadata: Option<String>,
    pub override_metadata: Option<String>,
    pub default_rule_set: Option<String>,
    pub override_rule_set: Option<String>,
}

impl ConfigExtra {
    /// Whether nothing is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// These settings with `newer`'s laid over them: what a PUT leaves.
    #[must_use]
    pub fn merged(&self, newer: &Self) -> Self {
        fn pick<T: Clone>(old: &Option<T>, new: &Option<T>) -> Option<T> {
            new.clone().or_else(|| old.clone())
        }
        Self {
            alias: pick(&self.alias, &newer.alias),
            normalize: pick(&self.normalize, &newer.normalize),
            validate_fields: pick(&self.validate_fields, &newer.validate_fields),
            compatibility_group: pick(&self.compatibility_group, &newer.compatibility_group),
            default_metadata: pick(&self.default_metadata, &newer.default_metadata),
            override_metadata: pick(&self.override_metadata, &newer.override_metadata),
            default_rule_set: pick(&self.default_rule_set, &newer.default_rule_set),
            override_rule_set: pick(&self.override_rule_set, &newer.override_rule_set),
        }
    }
}

/// A subject's (or the global) configuration as `GET /config` answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigView {
    pub level: Level,
    pub extra: ConfigExtra,
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

    /// Whether a subject's schemas are normalized when a request does not ask: its own setting, else the global one.
    pub(super) fn normalizes(&self, subject: &str) -> bool {
        let own = (!subject.is_empty())
            .then(|| self.extras.get(subject).and_then(|e| e.normalize))
            .flatten();
        own.or_else(|| self.extras.get("").and_then(|e| e.normalize))
            .unwrap_or(false)
    }

    /// The configuration a subject has of its own (`""`: the global one, which always has one).
    fn own_config(&self, subject: &str) -> Option<ConfigView> {
        let level = self.levels.get(subject);
        let extra = self.extras.get(subject);
        if !subject.is_empty() && level.is_none() && extra.is_none() {
            return None;
        }
        Some(ConfigView {
            level: level
                .and_then(|s| Level::parse(&s.value))
                .unwrap_or(Level::Backward),
            extra: extra.cloned().unwrap_or_default(),
        })
    }

    /// The mode a write to a subject is held to: the subject's, unless the global one is READONLY_OVERRIDE.
    pub(super) fn enforced_mode(&self, subject: &str) -> Mode {
        let global = self.mode_of("");
        if global == Mode::ReadOnlyOverride {
            return global;
        }
        self.mode_of(subject)
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
        self.set_config(subject, Some(level), &ConfigExtra::default())
    }

    /// The configuration of a subject (`""`: the global one): its level and the settings beside it. With `fallback`, a
    /// subject without one of its own reports the global one.
    ///
    /// # Errors
    /// The subject has no configuration of its own and `fallback` is off.
    pub fn config(&self, subject: &str, fallback: bool) -> Result<ConfigView, RegistryError> {
        let s = self.lock();
        match s.own_config(subject) {
            Some(c) => Ok(c),
            None if fallback => s
                .own_config("")
                .ok_or_else(|| RegistryError::subject_not_found("")),
            None => Err(RegistryError::new(
                404,
                code::SUBJECT_COMPAT_NOT_SET,
                format!("Subject '{subject}' does not have subject-level compatibility configured"),
            )),
        }
    }

    /// Sets the level and settings of a subject (`""`: the global one); what is not given stays as it was. A subject's
    /// first configuration without a level takes BACKWARD, as Confluent's does.
    ///
    /// # Errors
    /// An invalid level or subject, a read-only subject, a store failure.
    pub fn set_config(
        &self,
        subject: &str,
        level: Option<&str>,
        extra: &ConfigExtra,
    ) -> Result<(), RegistryError> {
        let level = match level {
            Some(l) => Some(Level::parse(l).ok_or_else(RegistryError::invalid_compatibility)?),
            None => None,
        };
        check_subject(subject)?;
        let mut s = self.lock();
        // A read-only subject's level does not change either (recorded: readonly-subject).
        if !subject.is_empty() && s.enforced_mode(subject).read_only() {
            return Err(RegistryError::read_only(subject));
        }
        let at = (self.clock)();
        let mut b = Batch::default();
        let level = level.or_else(|| {
            (!subject.is_empty() && !s.levels.contains_key(subject)).then_some(Level::Backward)
        });
        let setting = level.map(|l| Setting {
            value: l.name().to_owned(),
            at,
        });
        if let Some(setting) = &setting {
            b.put(&Key::Level(subject.to_owned()), setting);
        }
        let merged = s
            .extras
            .get(subject)
            .cloned()
            .unwrap_or_default()
            .merged(extra);
        let extra_changed = !extra.is_empty();
        if extra_changed {
            b.put(&Key::Config(subject.to_owned()), &merged);
        }
        self.commit(&b)?;
        if let Some(setting) = setting {
            s.levels.insert(subject.to_owned(), setting);
        }
        if extra_changed {
            s.extras.insert(subject.to_owned(), merged);
        }
        Ok(())
    }

    /// Removes a subject's own level (the global one goes back to the default) and returns what applied before.
    ///
    /// # Errors
    /// The subject has no level of its own; a store failure.
    pub fn delete_level(&self, subject: &str) -> Result<Level, RegistryError> {
        self.delete_config(subject).map(|c| c.level)
    }

    /// Removes a subject's own configuration (the global one goes back to the default) and returns it as it was.
    ///
    /// # Errors
    /// The subject has no configuration of its own; a store failure.
    pub fn delete_config(&self, subject: &str) -> Result<ConfigView, RegistryError> {
        let mut s = self.lock();
        let before = s
            .own_config(subject)
            .ok_or_else(|| RegistryError::subject_not_found(subject))?;
        let mut b = Batch::default();
        b.delete(&Key::Level(subject.to_owned()));
        b.delete(&Key::Config(subject.to_owned()));
        self.commit(&b)?;
        s.levels.remove(subject);
        s.extras.remove(subject);
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

    /// Sets the mode of a subject (`""`: the global one). IMPORT is refused where schemas are already registered (in the
    /// subject, or anywhere for the global mode) unless `force`, as Confluent refuses it.
    ///
    /// # Errors
    /// An invalid mode or subject, IMPORT over existing schemas, a store failure.
    pub fn set_mode(&self, subject: &str, mode: &str, force: bool) -> Result<Mode, RegistryError> {
        let mode = Mode::parse(mode).ok_or_else(RegistryError::invalid_mode)?;
        check_subject(subject)?;
        let mut s = self.lock();
        let occupied = if subject.is_empty() {
            s.subjects.values().any(|vs| !vs.is_empty())
        } else {
            !s.versions(subject).is_empty()
        };
        if mode == Mode::Import && occupied && !force {
            return Err(RegistryError::new(
                422,
                code::OPERATION_NOT_PERMITTED,
                "Cannot import since found existing subjects",
            ));
        }
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
            // Confluent's answer (tools/registry-diff): no such subject, whatever versions it has.
            return Err(RegistryError::subject_not_found(subject));
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
