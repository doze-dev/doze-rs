//! Erasing the registry, for the broker's Erase everything (docs/port/data-control.md): every subject (soft-deleted ones
//! too), every schema, and each subject's own level and mode go, in one write. The global level and mode stay (they are the
//! registry's settings, as the broker's configs are the broker's), and so does the next id: a client keeps what an id meant
//! for as long as it runs, so an id is never given to a second schema.

use super::Registry;
use crate::error::RegistryError;
use crate::store::{Batch, Key};

/// What erasing the registry removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Erased {
    /// Subjects, soft-deleted ones included.
    pub subjects: usize,
    pub schemas: usize,
}

impl Registry {
    /// Removes every subject and schema; keeps the global level and mode and the next id.
    ///
    /// # Errors
    /// The store refused the write; nothing changed.
    pub fn erase(&self) -> Result<Erased, RegistryError> {
        let mut s = self.lock();
        let mut b = Batch::default();
        for (subject, versions) in &s.subjects {
            for v in versions {
                b.delete(&Key::Version(subject.clone(), v.number));
            }
        }
        for id in s.schemas.keys() {
            b.delete(&Key::Schema(*id));
        }
        for subject in s.levels.keys().filter(|k| !k.is_empty()) {
            b.delete(&Key::Level(subject.clone()));
        }
        for subject in s.modes.keys().filter(|k| !k.is_empty()) {
            b.delete(&Key::Mode(subject.clone()));
        }
        for subject in s.extras.keys().filter(|k| !k.is_empty()) {
            b.delete(&Key::Config(subject.clone()));
        }
        // Read back at open as the largest schema's id plus one, which would go back to 1 with no schema left.
        b.put(&Key::NextId, &s.next_id);
        self.commit(&b)?;
        let erased = Erased {
            subjects: s.subjects.len(),
            schemas: s.schemas.len(),
        };
        s.subjects.clear();
        s.schemas.clear();
        s.by_print.clear();
        s.levels.retain(|k, _| k.is_empty());
        s.modes.retain(|k, _| k.is_empty());
        s.extras.retain(|k, _| k.is_empty());
        Ok(erased)
    }
}

#[cfg(test)]
mod tests {
    use crate::Registry;
    use crate::format::SchemaType;
    use crate::registry::{Origin, RegisterResult, SchemaInput};
    use crate::store::MemoryStorage;

    fn register(r: &Registry, subject: &str, text: &str) -> u32 {
        let input = SchemaInput::of(SchemaType::Avro, text);
        match r.register(subject, &input, false, &Origin::default()) {
            Ok(RegisterResult::Id(id)) => id,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn erasing_leaves_no_subject_keeps_the_global_level_and_never_gives_an_id_twice() {
        let store = MemoryStorage::default();
        let r = Registry::open(Box::new(store.clone())).unwrap();
        let first = register(&r, "orders-value", r#""string""#);
        register(&r, "users-value", r#""int""#);
        r.set_level("", "FULL").unwrap();
        r.set_level("orders-value", "NONE").unwrap();
        r.delete_subject("users-value", false).unwrap();
        let erased = r.erase().unwrap();
        assert_eq!((erased.subjects, erased.schemas), (2, 2));
        assert!(r.subjects("", true, false).is_empty());
        assert_eq!(
            r.level_for("orders-value", None),
            "FULL",
            "the global level stays"
        );
        // After a restart, as before it: nothing there, and the next schema gets an id never given before.
        let again = Registry::open(Box::new(store)).unwrap();
        assert!(again.subjects("", true, false).is_empty());
        assert!(register(&again, "orders-value", r#""long""#) > first + 1);
    }
}
