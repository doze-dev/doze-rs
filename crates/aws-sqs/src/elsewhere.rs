//! A queue the request's Region does not have, though another Region of this stack does (doze only: AWS keeps a queue in the Region
//! that made it, and so does doze). The error answers as before, with a sentence added that says where the queue is, so a client whose
//! Region differs from the one the queue was declared in is told so instead of being left to wonder.

use crate::Sqs;
use crate::core::{Answer, Ask};

impl Sqs {
    /// When `answer` says the queue it was asked for is missing, looks for the name in the other Regions that hold data and, if
    /// one has it, adds that to the message. Nothing else of the answer changes; the lookup runs on this path alone.
    pub(crate) async fn hint_regions(&self, region: &str, answer: &mut Answer) {
        let Some(name) = answer.missed.take() else {
            return;
        };
        let Err(error) = &mut answer.reply else {
            return;
        };
        let mut found = Vec::new();
        for key in self.actors.keys_on_disk() {
            if key == region {
                continue;
            }
            let Ok(actor) = self.actors.actor(&key) else {
                continue;
            };
            // A look at no messages: it answers whether the queue is there and changes nothing.
            let asked = actor
                .call(Ask::Peek {
                    name: name.clone(),
                    max: 0,
                })
                .await;
            if asked.is_ok_and(|a| a.reply.is_ok()) {
                found.push(key);
            }
        }
        if !found.is_empty() {
            error.note_other_regions("queue", &name, &found, region);
        }
    }
}
