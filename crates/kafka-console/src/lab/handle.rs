//! The Lab as the console holds it: the broker's fault table, and the short list of rehearsals that happened once.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use kafka_server::fault::FaultTable;

use crate::model::lab::{LabOnce, LabOnceKind};

/// The most rehearsals that happened once that are kept.
pub const MOST_ONCE: usize = 200;

/// The Lab. Present only when the feature is on (`--lab`, the default); a console without it has none, and every Lab page says so.
#[derive(Debug)]
pub struct Lab {
    table: Arc<FaultTable>,
    once: Mutex<VecDeque<LabOnce>>,
    next: AtomicU32,
}

impl Lab {
    /// The Lab over the table the server was given.
    #[must_use]
    pub fn new(table: Arc<FaultTable>) -> Self {
        Self {
            table,
            once: Mutex::new(VecDeque::new()),
            next: AtomicU32::new(1),
        }
    }

    /// The table of rules.
    #[must_use]
    pub fn table(&self) -> &Arc<FaultTable> {
        &self.table
    }

    /// Notes a rehearsal that happened once, and the group or topic it was about; the oldest are forgotten past [`MOST_ONCE`].
    pub fn note(&self, kind: LabOnceKind, sentence: String, ok: bool, subject: Option<String>) {
        let at_ms = self.table.now_ms();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut once = self.once.lock().unwrap_or_else(PoisonError::into_inner);
        if once.len() == MOST_ONCE {
            once.pop_back();
        }
        once.push_front(LabOnce {
            id,
            at_ms,
            kind,
            sentence,
            ok,
            subject,
        });
    }

    /// The rehearsals that happened once, newest first.
    #[must_use]
    pub fn once(&self) -> Vec<LabOnce> {
        let once = self.once.lock().unwrap_or_else(PoisonError::into_inner);
        once.iter().cloned().collect()
    }

    /// Forgets the rehearsals that happened once, and the rules that are over.
    pub fn clear_history(&self) {
        self.once
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.table.clear_ended();
    }
}
