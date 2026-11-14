//! A table of the names a ring's lines carry (instances, streams), so each line holds a number, not a string.

use std::collections::HashMap;
use std::mem::size_of;

/// Names by number, and numbers by name. Number 0 is the empty name.
#[derive(Debug)]
pub(super) struct Names {
    names: Vec<Box<str>>,
    ids: HashMap<Box<str>, u32>,
}

impl Default for Names {
    fn default() -> Self {
        let empty: Box<str> = Box::from("");
        Names {
            names: vec![empty.clone()],
            ids: HashMap::from([(empty, 0)]),
        }
    }
}

impl Names {
    /// The number of `name`, given one if it has none. Past `u32::MAX` names every new one is the empty name.
    pub(super) fn id(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let Ok(id) = u32::try_from(self.names.len()) else {
            return 0;
        };
        let name: Box<str> = Box::from(name);
        self.names.push(name.clone());
        self.ids.insert(name, id);
        id
    }

    /// The number of `name`, if it has one.
    pub(super) fn find(&self, name: &str) -> Option<u32> {
        self.ids.get(name).copied()
    }

    /// The name numbered `id`.
    pub(super) fn name(&self, id: u32) -> &str {
        self.names.get(id as usize).map_or("", |n| n)
    }

    pub(super) fn heap_bytes(&self) -> usize {
        let text: usize = self.names.iter().map(|n| n.len() * 2).sum();
        text + self.names.capacity() * size_of::<Box<str>>()
            + self.ids.capacity() * (size_of::<Box<str>>() + size_of::<u32>() + 1)
    }
}
