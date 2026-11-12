//! Deleting: `DeleteSecret` marks a secret for deletion (or removes it at once), `RestoreSecret` undoes the mark. The sweep in
//! [`Core`] removes the secret when its window has run out.

use aws_model_secrets_manager::keys::{
    DeleteSecretRequest as Delete, DeleteSecretResponse as Deleted,
    RestoreSecretRequest as Restore, RestoreSecretResponse as Restored,
};
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_parameter, not_found};
use crate::model::DAY_MS;

const DEFAULT_WINDOW_DAYS: i64 = 30;

#[allow(clippy::cast_precision_loss)]
fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

impl Core {
    pub(crate) fn delete_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let days = v.get(Delete::RECOVERY_WINDOW_IN_DAYS);
        let force = v
            .get(Delete::FORCE_DELETE_WITHOUT_RECOVERY)
            .unwrap_or(false);
        // Recorded 2026-11-09, in this order: the two together are refused before the window is looked at.
        if force && days.is_some() {
            return Err(invalid_parameter(
                "You can't use ForceDeleteWithoutRecovery in conjunction with RecoveryWindowInDays.",
            ));
        }
        if days.is_some_and(|d| !(7..=30).contains(&d)) {
            return Err(invalid_parameter(
                "The RecoveryWindowInDays value must be between 7 and 30 days (inclusive).",
            ));
        }
        let name = self.locate(v.get(Delete::SECRET_ID).unwrap_or_default())?;
        let now = self.now_ms();
        let secret = self.secrets.get(&name).ok_or_else(not_found)?;
        let (arn, existing) = (secret.arn.clone(), secret.purge_ms);
        let due = if force {
            self.secrets.remove(&name);
            now
        } else if let Some(at) = existing {
            // Already marked: the same answer again (not recorded).
            at
        } else {
            let at = now + days.unwrap_or(DEFAULT_WINDOW_DAYS) * DAY_MS;
            if let Some(s) = self.secrets.get_mut(&name) {
                s.deleted_ms = Some(now);
                s.purge_ms = Some(at);
            }
            self.purge_at(at);
            at
        };
        reply::<Deleted>(|o| {
            o.put(Deleted::ARN, &arn);
            o.put(Deleted::NAME, &name);
            o.put_time(Deleted::DELETION_DATE, seconds(due));
        })
    }

    pub(crate) fn restore_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Restore>(call)?;
        let name = self.locate(v.get(Restore::SECRET_ID).unwrap_or_default())?;
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        secret.deleted_ms = None;
        secret.purge_ms = None;
        let arn = secret.arn.clone();
        reply::<Restored>(|o| {
            o.put(Restored::ARN, &arn);
            o.put(Restored::NAME, &name);
        })
    }
}
