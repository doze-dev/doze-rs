//! Key rotation: the setting, and `RotateKeyOnDemand`. Rotation adds a generation of material; the earlier ones stay, so what they
//! sealed still opens. Nothing rotates by itself: the schedule is recorded and reported, and a rotation happens when it is asked
//! for (decision: SCOPE.md, "NOT NEEDED").

use aws_model_kms::keys::{
    DisableKeyRotationRequest as Disable, EnableKeyRotationRequest as Enable,
    GetKeyRotationStatusRequest as Status, GetKeyRotationStatusResponse as Statused,
    ListKeyRotationsRequest as List, ListKeyRotationsResponse as Listed,
    RotateKeyOnDemandRequest as Rotate, RotateKeyOnDemandResponse as Rotated, RotationsListEntry,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::find::not_found_key;
use crate::keys::{invalid_state, seconds};
use crate::model::{DAY_MS, Key, State};
use crate::spec::Spec;

const DEFAULT_DAYS: i64 = 365;
const LIST_PAGE: usize = 100;

fn only_symmetric(key: &Key) -> Result<(), ApiError> {
    if key.state == State::PendingDeletion {
        return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
    }
    if key.spec != Spec::Symmetric {
        // (memory)
        return Err(ApiError::modeled(
            "UnsupportedOperationException",
            format!(
                "{} is not a symmetric encryption key: only those rotate.",
                key.arn
            ),
        ));
    }
    Ok(())
}

impl Core {
    pub(crate) fn enable_key_rotation(&mut self, call: &Call) -> Reply {
        let v = request::<Enable>(call)?;
        let days = v
            .get(Enable::ROTATION_PERIOD_IN_DAYS)
            .unwrap_or(DEFAULT_DAYS);
        if !(90..=2560).contains(&days) {
            return Err(validation(
                "RotationPeriodInDays must be between 90 and 2560",
            ));
        }
        let now = self.now_ms();
        let id = self.locate(call, v.get(Enable::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        only_symmetric(key)?;
        key.rotation_enabled = true;
        key.rotation_days = days;
        key.next_rotation_ms = Some(now + days * DAY_MS);
        empty()
    }

    pub(crate) fn disable_key_rotation(&mut self, call: &Call) -> Reply {
        let v = request::<Disable>(call)?;
        let id = self.locate(call, v.get(Disable::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        only_symmetric(key)?;
        key.rotation_enabled = false;
        key.next_rotation_ms = None;
        empty()
    }

    pub(crate) fn get_key_rotation_status(&mut self, call: &Call) -> Reply {
        let v = request::<Status>(call)?;
        let key = self.key(call, v.get(Status::KEY_ID).unwrap_or_default())?;
        reply::<Statused>(|o| {
            o.put_bool(Statused::KEY_ROTATION_ENABLED, key.rotation_enabled);
            o.put(Statused::KEY_ID, &key.arn);
            if key.rotation_enabled {
                o.put_int(Statused::ROTATION_PERIOD_IN_DAYS, key.rotation_days);
                if let Some(at) = key.next_rotation_ms {
                    o.put_time(Statused::NEXT_ROTATION_DATE, seconds(at));
                }
            }
        })
    }

    pub(crate) fn rotate_key_on_demand(&mut self, call: &Call) -> Reply {
        let v = request::<Rotate>(call)?;
        let id = self.locate(call, v.get(Rotate::KEY_ID).unwrap_or_default())?;
        let now = self.now_ms();
        let key = self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))?;
        only_symmetric(key)?;
        let fresh = self.new_material(key.spec, now)?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        if let Some(old) = key.materials.last_mut() {
            old.retired_by = Some("ON_DEMAND".to_owned());
            old.retired_ms = Some(now);
        }
        key.materials.push(fresh);
        if key.rotation_enabled {
            key.next_rotation_ms = Some(now + key.rotation_days * DAY_MS);
        }
        let arn = key.arn.clone();
        reply::<Rotated>(|o| {
            o.put(Rotated::KEY_ID, &arn);
        })
    }

    /// The rotations a key has had: for each generation that was replaced, the one that replaced it (not recorded).
    pub(crate) fn list_key_rotations(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::LIMIT)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let skip: usize = v
            .get(List::MARKER)
            .and_then(|m| m.parse().ok())
            .unwrap_or(0);
        let key = self.key(call, v.get(List::KEY_ID).unwrap_or_default())?;
        let rows: Vec<_> = key
            .materials
            .windows(2)
            .filter_map(|w| Some((w.first()?, w.get(1)?)))
            .collect();
        let more = rows.len() > skip + limit;
        let page: Vec<_> = rows.into_iter().skip(skip).take(limit).collect();
        reply::<Listed>(|o| {
            o.put_list(Listed::ROTATIONS, |l| {
                for (old, new) in &page {
                    l.push(|e: &mut Object<'_, RotationsListEntry>| {
                        e.put(RotationsListEntry::KEY_ID, &key.arn);
                        e.put(RotationsListEntry::KEY_MATERIAL_ID, &new.id);
                        e.put_time(
                            RotationsListEntry::ROTATION_DATE,
                            seconds(old.retired_ms.unwrap_or(new.created_ms)),
                        );
                        e.put(
                            RotationsListEntry::ROTATION_TYPE,
                            old.retired_by.as_deref().unwrap_or("ON_DEMAND"),
                        );
                    });
                }
            });
            o.put_bool(Listed::TRUNCATED, more);
            if more {
                o.put(Listed::NEXT_MARKER, &(skip + limit).to_string());
            }
        })
    }
}
