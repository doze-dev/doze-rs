//! The credential report: one CSV row for the root and for each user, in the columns and words of the IAM user guide's "Getting
//! credential reports" (memory: not seen recorded). AWS makes at most one report every four hours: a `GenerateCredentialReport`
//! within four hours of the last report answers `COMPLETE` and makes nothing new, as here. The report is made at once, where AWS
//! takes a while (`STARTED`, then `INPROGRESS`).

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::held::{CredKind, Pref, Report};
use crate::model::{AccessKey, Entity};
use crate::req::secs;

const KEY: &str = "credential";
/// How long a report lasts, and how often one is made.
const FOUR_HOURS_MS: i64 = 4 * 3600 * 1000;
/// When IAM first ran, under the preferences.
pub(crate) const CREATED: &str = "created";

const HEADER: &str = "user,arn,user_creation_time,password_enabled,password_last_used,password_last_changed,password_next_rotation,mfa_active,access_key_1_active,access_key_1_last_rotated,access_key_1_last_used_date,access_key_1_last_used_region,access_key_1_last_used_service,access_key_2_active,access_key_2_last_rotated,access_key_2_last_used_date,access_key_2_last_used_region,access_key_2_last_used_service,cert_1_active,cert_1_last_rotated,cert_2_active,cert_2_last_rotated";

const NA: &str = "N/A";

/// A time as the report writes it: `2026-11-18T09:30:00+00:00`.
fn time(ms: i64) -> String {
    let iso = aws_core::time::format_iso8601(ms.div_euclid(1000));
    format!("{}+00:00", iso.get(..19).unwrap_or(&iso))
}

fn flag(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

impl Core {
    /// When IAM first ran here: the root's creation time in the report. Noted the first time it is asked.
    pub(crate) fn created_ms(&mut self) -> i64 {
        if let Some(Pref::Created(at)) = self.held.prefs.get(CREATED) {
            return *at;
        }
        let now = self.now_ms();
        self.held
            .prefs
            .insert(CREATED.to_owned(), Pref::Created(now));
        now
    }

    pub(crate) fn generate_credential_report(&mut self, account: &str) -> Value {
        let now = self.now_ms();
        if let Some(r) = self.held.reports.get(KEY)
            && now - r.generated_ms < FOUR_HOURS_MS
        {
            return json!({"State": "COMPLETE"});
        }
        let csv = self.credential_csv(account);
        self.held.reports.insert(
            KEY.to_owned(),
            Report {
                generated_ms: now,
                csv,
            },
        );
        // (memory) AWS's words when it starts a report.
        json!({"State": "STARTED", "Description": "No report exists. Starting a new report generation task"})
    }

    pub(crate) fn get_credential_report(&self) -> Result<Value, ApiError> {
        let r = self.held.reports.get(KEY).ok_or_else(|| {
            // (memory)
            ApiError::modeled(
                "CredentialReportNotPresentException",
                "Credential report not present. Call GenerateCredentialReport first.",
            )
        })?;
        if self.now_ms() - r.generated_ms >= FOUR_HOURS_MS {
            // (memory)
            return Err(ApiError::modeled(
                "CredentialReportExpiredException",
                "The credential report is more than four hours old. Call GenerateCredentialReport for a new one.",
            ));
        }
        Ok(json!({
            "Content": aws_core::base64::encode(r.csv.as_bytes()),
            "ReportFormat": "text/csv",
            "GeneratedTime": secs(r.generated_ms),
        }))
    }

    fn credential_csv(&mut self, account: &str) -> String {
        let created = self.created_ms();
        let mut lines = vec![HEADER.to_owned(), root_row(account, created)];
        lines.extend(
            self.entities
                .with_prefix("user/")
                .map(|(_, u)| self.user_row(u)),
        );
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    fn user_row(&self, u: &Entity) -> String {
        let password = self.held.passwords.get(&u.name);
        let has_password = u.password_set_ms.is_some();
        let changed = password
            .map(|p| p.changed_ms)
            .or(u.password_set_ms)
            .filter(|_| has_password);
        let mut keys: Vec<&AccessKey> = self
            .keys
            .iter()
            .map(|(_, k)| k)
            .filter(|k| k.user == u.name)
            .collect();
        keys.sort_by_key(|k| k.created_ms);
        let certs: Vec<(bool, i64)> = self
            .signing_of(&u.name)
            .filter(|c| matches!(c.kind, CredKind::Signing { .. }))
            .map(|c| (c.active, c.created_ms))
            .collect();
        let mut row = vec![
            u.name.clone(),
            u.arn.clone(),
            time(u.created_ms),
            flag(has_password).to_owned(),
            // No console sign-in is made locally, so a password is never used.
            if has_password { "no_information" } else { NA }.to_owned(),
            changed.map_or_else(|| NA.to_owned(), time),
            self.next_rotation(changed),
            flag(self.devices_of(&u.name).next().is_some()).to_owned(),
        ];
        for i in 0..2 {
            row.extend(key_columns(keys.get(i).copied()));
        }
        for i in 0..2 {
            row.extend(cert_columns(certs.get(i).copied()));
        }
        row.join(",")
    }

    /// When the password must next be changed, under the policy's `MaxPasswordAge`.
    fn next_rotation(&self, changed: Option<i64>) -> String {
        let days = self
            .account()
            .password_policy
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|p| p.get("MaxPasswordAge").and_then(Value::as_i64));
        match (changed, days) {
            (Some(at), Some(days)) if days > 0 => time(at + days * 86_400_000),
            _ => NA.to_owned(),
        }
    }
}

/// The root's row (memory: AWS writes `not_supported` for what the root cannot have through IAM).
fn root_row(account: &str, created: i64) -> String {
    let mut row = vec![
        "<root_account>".to_owned(),
        format!("arn:aws:iam::{account}:root"),
        time(created),
        "not_supported".to_owned(),
        "no_information".to_owned(),
        "not_supported".to_owned(),
        "not_supported".to_owned(),
        "false".to_owned(),
    ];
    for _ in 0..2 {
        row.extend(key_columns(None));
    }
    for _ in 0..2 {
        row.extend(cert_columns(None));
    }
    row.join(",")
}

fn key_columns(k: Option<&AccessKey>) -> [String; 5] {
    let Some(k) = k else {
        return ["false".into(), NA.into(), NA.into(), NA.into(), NA.into()];
    };
    let (used, region, service) = k.last_used.as_ref().map_or_else(
        || (NA.to_owned(), NA.to_owned(), NA.to_owned()),
        |(at, service, region)| (time(*at), region.clone(), service.clone()),
    );
    [
        flag(k.active).into(),
        time(k.created_ms),
        used,
        region,
        service,
    ]
}

fn cert_columns(c: Option<(bool, i64)>) -> [String; 2] {
    c.map_or_else(
        || ["false".into(), NA.into()],
        |(active, at)| [flag(active).into(), time(at)],
    )
}
