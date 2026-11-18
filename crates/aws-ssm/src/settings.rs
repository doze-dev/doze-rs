//! Service settings: `GetServiceSetting`, `UpdateServiceSetting`, `ResetServiceSetting`. The ids and the values each takes are the
//! model's (its documentation of `SettingId` and `SettingValue`); the defaults are AWS's as its user guide gives them (memory). A
//! setting holds its default until it is updated, and only an updated one is stored. One of them changes what this service does:
//! `/ssm/parameter-store/default-parameter-tier` is the tier of a parameter written without one. The others are kept and answered.

use aws_model_ssm::keys::{
    GetServiceSettingRequest as Get, GetServiceSettingResult as Got,
    ResetServiceSettingRequest as Reset, ResetServiceSettingResult as Reseted,
    ServiceSetting as Shown, UpdateServiceSettingRequest as Update,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use serde::{Deserialize, Serialize};

use crate::core::{Core, validation};
use crate::read::seconds;
use aws_wire::handler::{empty, reply, request};

/// A setting someone changed from its default.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Setting {
    pub(crate) value: String,
    pub(crate) modified_ms: i64,
    pub(crate) modified_by: String,
}

/// The default tier setting: its id.
pub(crate) const DEFAULT_TIER: &str = "/ssm/parameter-store/default-parameter-tier";

/// Each setting AWS defines: its id, its default (none: it has no value until one is set), and the values it takes (none: any).
const SETTINGS: &[(&str, Option<&str>, &[&str])] = &[
    (
        "/ssm/appmanager/appmanager-enabled",
        Some("False"),
        &["True", "False"],
    ),
    (
        "/ssm/automation/customer-script-log-destination",
        Some("None"),
        &["CloudWatch", "None"],
    ),
    (
        "/ssm/automation/customer-script-log-group-name",
        Some("/aws/ssm/automation/executeScript"),
        &[],
    ),
    (
        "/ssm/automation/enable-adaptive-concurrency",
        Some("False"),
        &["True", "False"],
    ),
    (
        "/ssm/documents/console/public-sharing-permission",
        Some("Enable"),
        &["Enable", "Disable"],
    ),
    (
        "/ssm/managed-instance/activation-tier",
        Some("standard"),
        &["standard", "advanced"],
    ),
    (
        "/ssm/managed-instance/default-ec2-instance-management-role",
        None,
        &[],
    ),
    (
        "/ssm/opsinsights/opscenter",
        Some("Disabled"),
        &["Enabled", "Disabled"],
    ),
    (
        DEFAULT_TIER,
        Some("Standard"),
        &["Standard", "Advanced", "Intelligent-Tiering"],
    ),
    (
        "/ssm/parameter-store/high-throughput-enabled",
        Some("false"),
        &["true", "false"],
    ),
];

/// The id a request names: an id, or the setting's ARN (`arn:aws:ssm:<region>:<account>:servicesetting/ssm/...`).
fn id_of(text: &str) -> &str {
    text.split_once(":servicesetting")
        .map_or(text, |(_, id)| id)
}

fn known(
    id: &str,
) -> Result<&'static (&'static str, Option<&'static str>, &'static [&'static str]), ApiError> {
    SETTINGS
        .iter()
        .find(|(s, _, _)| *s == id)
        .ok_or_else(|| not_found(id))
}

fn not_found(id: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "ServiceSettingNotFound",
        format!("The service setting {id} was not found. Check the setting ID and try again."),
    )
}

/// What a setting is now: its value, when and by whom it was set, and whether it is the default.
struct Now<'a> {
    id: &'a str,
    value: &'a str,
    modified: Option<(i64, &'a str)>,
}

impl Core {
    /// The value of setting `id` now (its default when it was not updated).
    pub(crate) fn setting(&self, id: &str) -> Option<String> {
        if let Some(s) = self.settings.get(id) {
            return Some(s.value.clone());
        }
        SETTINGS
            .iter()
            .find(|(s, _, _)| *s == id)
            .and_then(|(_, default, _)| default.map(str::to_owned))
    }

    fn now_of<'a>(&'a self, id: &'a str) -> Result<Now<'a>, ApiError> {
        let (_, default, _) = known(id)?;
        if let Some(s) = self.settings.get(id) {
            return Ok(Now {
                id,
                value: &s.value,
                modified: Some((s.modified_ms, &s.modified_by)),
            });
        }
        let value = default.ok_or_else(|| not_found(id))?;
        Ok(Now {
            id,
            value,
            modified: None,
        })
    }

    pub(crate) fn get_service_setting(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let id = id_of(v.get(Get::SETTING_ID).unwrap_or_default());
        let now = self.now_of(id)?;
        reply::<Got>(|o| {
            o.put_object(Got::SERVICE_SETTING, |s| shown(s, call, &now));
        })
    }

    pub(crate) fn update_service_setting(&mut self, call: &Call) -> Reply {
        let v = request::<Update>(call)?;
        let id = id_of(v.get(Update::SETTING_ID).unwrap_or_default());
        let value = v.get(Update::SETTING_VALUE).unwrap_or_default();
        let (_, _, allowed) = known(id)?;
        if !allowed.is_empty() && !allowed.contains(&value) {
            // (memory)
            return Err(validation(format!(
                "The value {value} is not valid for the setting {id}. Valid values: {}.",
                allowed.join(", ")
            )));
        }
        let setting = Setting {
            value: value.to_owned(),
            modified_ms: self.now_ms(),
            modified_by: format!("arn:aws:iam::{}:root", call.caller.account_id),
        };
        self.settings.insert(id.to_owned(), setting);
        empty()
    }

    pub(crate) fn reset_service_setting(&mut self, call: &Call) -> Reply {
        let v = request::<Reset>(call)?;
        let id = id_of(v.get(Reset::SETTING_ID).unwrap_or_default());
        known(id)?;
        self.settings.remove(id);
        let now = self.now_of(id)?;
        reply::<Reseted>(|o| {
            o.put_object(Reseted::SERVICE_SETTING, |s| shown(s, call, &now));
        })
    }
}

fn shown(s: &mut Object<'_, Shown>, call: &Call, now: &Now<'_>) {
    s.put(Shown::SETTING_ID, now.id);
    s.put(Shown::SETTING_VALUE, now.value);
    s.put(
        Shown::ARN,
        &format!(
            "arn:aws:ssm:{}:{}:servicesetting{}",
            call.caller.region, call.caller.account_id, now.id
        ),
    );
    match now.modified {
        Some((at, by)) => {
            s.put_time(Shown::LAST_MODIFIED_DATE, seconds(at));
            s.put(Shown::LAST_MODIFIED_USER, by);
            s.put(Shown::STATUS, "Customized");
        }
        None => {
            // A default has a modification date too (recorded: test_ssm.py::test_service_settings; its value, and who AWS
            // says set it, are AWS's own). doze-aws gives the fixed date its public parameters carry, set by the system (memory).
            s.put_time(
                Shown::LAST_MODIFIED_DATE,
                seconds(crate::public::MODIFIED_MS),
            );
            s.put(Shown::LAST_MODIFIED_USER, "System");
            s.put(Shown::STATUS, "Default");
        }
    }
}
