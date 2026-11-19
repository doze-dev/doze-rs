//! Virtual MFA devices: made with a seed an authenticator app takes, enabled for a user with two consecutive codes, and asked for a
//! code when STS makes a session with `SerialNumber` and `TokenCode`. The codes are real TOTP (`totp`): any authenticator app, or
//! `oathtool --totp -b <seed>`, gives the codes doze-aws accepts. Hardware tokens and FIDO keys cannot be registered locally.
//!
//! Wording marked (memory) was not seen recorded.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::entity::arn_of;
use crate::errors::{exists, limit, no_such};
use crate::held::Mfa;
use crate::model::Kind;
use crate::req::{Req, page, secs, tag_list};
use crate::totp;

/// How many MFA devices one user may have (memory: AWS raised it to eight in 2022).
const PER_USER: usize = 8;
const MAX_TAGS: usize = 50;
/// How far back two codes may be when a device is enabled: AWS says late codes still enable it, out of step (memory: the window).
const ENABLE_BACK: i64 = 10;
/// How far either way two codes may be to bring a device back in step (memory).
const RESYNC_SPAN: i64 = 60;

fn no_device(serial: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("MFA Device with serial number {serial} does not exist."),
    )
}

fn bad_codes() -> ApiError {
    // (memory)
    ApiError::modeled(
        "InvalidAuthenticationCodeException",
        "Authentication code for device is not valid.",
    )
}

impl Core {
    /// The user a call is about: the one it names, or the IAM user whose credentials signed it; `None` for the root or a role.
    pub(crate) fn named_or_caller(&self, r: &Req<'_>) -> Result<Option<String>, ApiError> {
        if let Some(name) = r.opt("UserName") {
            self.entity(Kind::User, name)?;
            return Ok(Some(name.to_owned()));
        }
        let arn = self.identify(r).arn;
        Ok(arn
            .split_once(":user/")
            .and_then(|(_, rest)| rest.rsplit('/').next())
            .filter(|name| self.entity(Kind::User, name).is_ok())
            .map(str::to_owned))
    }

    fn device(&self, serial: &str) -> Result<&Mfa, ApiError> {
        self.held.mfa.get(serial).ok_or_else(|| no_device(serial))
    }

    /// The device `serial` enabled for `user`.
    fn users_device(&self, user: &str, serial: &str) -> Result<&Mfa, ApiError> {
        self.device(serial)
            .ok()
            .filter(|d| d.user.as_ref().is_some_and(|(u, _)| u == user))
            .ok_or_else(|| no_device(serial))
    }

    pub(crate) fn create_virtual_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let path = r.opt("Path").unwrap_or("/");
        let serial = arn_of(r.account(), "mfa", path, r.s("VirtualMFADeviceName"));
        if self.held.mfa.get(&serial).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "EntityAlreadyExistsException",
                "MFADevice entity at the same path and name already exists.",
            ));
        }
        let tags: BTreeMap<String, String> = r.tags().into_iter().collect();
        // Recorded 2026-11-19: AWS's seed is 40 bytes (64 base32 characters).
        let seed = aws_seal::random_bytes::<40>()
            .ok_or_else(|| ApiError::internal("no randomness"))?
            .to_vec();
        let mut shown = Map::new();
        shown.insert("SerialNumber".into(), json!(serial));
        // A blob on the wire: the base32 text, base64-encoded.
        shown.insert(
            "Base32StringSeed".into(),
            json!(aws_core::base64::encode(totp::base32(&seed).as_bytes())),
        );
        if !tags.is_empty() {
            shown.insert("Tags".into(), tag_list(tags.iter()));
        }
        let device = Mfa {
            serial: serial.clone(),
            seed,
            created_ms: self.now_ms(),
            user: None,
            last_step: 0,
            tags,
        };
        self.held.mfa.insert(serial, device);
        Ok(json!({"VirtualMFADevice": shown}))
    }

    pub(crate) fn enable_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName");
        self.entity(Kind::User, user)?;
        let serial = r.s("SerialNumber");
        let d = self.device(serial)?;
        if d.user.is_some() {
            // (memory)
            return Err(exists("MFA Device", serial));
        }
        if self.devices_of(user).count() >= PER_USER {
            return Err(limit(format!(
                "Cannot exceed quota for MFADevicesPerUser: {PER_USER}"
            )));
        }
        let now = totp::step_of(self.now_ms());
        let step = totp::consecutive(
            &d.seed,
            (now - ENABLE_BACK)..=(now + 1),
            r.s("AuthenticationCode1"),
            r.s("AuthenticationCode2"),
        )
        .filter(|s| *s > d.last_step)
        .ok_or_else(bad_codes)?;
        let at = self.now_ms();
        if let Some(d) = self.held.mfa.get_mut(serial) {
            d.user = Some((user.to_owned(), at));
            d.last_step = step + 1;
        }
        Ok(Value::Null)
    }

    pub(crate) fn resync_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = r.s("UserName");
        self.entity(Kind::User, user)?;
        let serial = r.s("SerialNumber");
        let d = self.users_device(user, serial)?;
        let now = totp::step_of(self.now_ms());
        let step = totp::consecutive(
            &d.seed,
            (now - RESYNC_SPAN)..=(now + RESYNC_SPAN),
            r.s("AuthenticationCode1"),
            r.s("AuthenticationCode2"),
        )
        .ok_or_else(bad_codes)?;
        if let Some(d) = self.held.mfa.get_mut(serial) {
            d.last_step = step + 1;
        }
        Ok(Value::Null)
    }

    pub(crate) fn deactivate_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let serial = r.s("SerialNumber");
        let user = self.named_or_caller(r)?.ok_or_else(|| no_device(serial))?;
        self.users_device(&user, serial)?;
        if let Some(d) = self.held.mfa.get_mut(serial) {
            d.user = None;
        }
        Ok(Value::Null)
    }

    pub(crate) fn delete_virtual_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let serial = r.s("SerialNumber");
        if self.device(serial)?.user.is_some() {
            // (memory)
            return Err(crate::errors::conflict(
                "MFA Device must be deactivated before it can be deleted.",
            ));
        }
        self.held.mfa.remove(serial);
        Ok(Value::Null)
    }

    /// The model: "For this API, we only accept FIDO security key ARNs." None can be registered locally, so every serial is unknown.
    pub(crate) fn get_mfa_device(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        if let Some(name) = r.opt("UserName") {
            self.entity(Kind::User, name)?;
        }
        Err(ApiError::modeled(
            "NoSuchEntityException",
            format!(
                "MFA device {} is not a FIDO security key: GetMFADevice answers for FIDO keys only, and none can be registered in doze-aws. ListMFADevices lists virtual devices.",
                r.s("SerialNumber")
            ),
        ))
    }

    /// The devices enabled for `user`.
    pub(crate) fn devices_of<'a>(&'a self, user: &'a str) -> impl Iterator<Item = &'a Mfa> + 'a {
        self.held
            .mfa
            .iter()
            .map(|(_, d)| d)
            .filter(move |d| d.user.as_ref().is_some_and(|(u, _)| u == user))
    }

    pub(crate) fn list_mfa_devices(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        // The root's devices are not IAM's to hold here: it has none.
        let items = match self.named_or_caller(r)? {
            Some(user) => self
                .devices_of(&user)
                .map(|d| json!({"UserName": user, "SerialNumber": d.serial, "EnableDate": secs(d.user.as_ref().map_or(0, |(_, at)| *at))}))
                .collect(),
            None => Vec::new(),
        };
        page(r, items, "MFADevices")
    }

    pub(crate) fn list_virtual_mfa_devices(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let status = r.opt("AssignmentStatus").unwrap_or("Any");
        let items = self
            .held
            .mfa
            .iter()
            .map(|(_, d)| d)
            .filter(|d| match status {
                "Assigned" => d.user.is_some(),
                "Unassigned" => d.user.is_none(),
                _ => true,
            })
            .map(|d| self.virtual_json(d))
            .collect();
        page(r, items, "VirtualMFADevices")
    }

    fn virtual_json(&self, d: &Mfa) -> Value {
        let mut m = Map::new();
        m.insert("SerialNumber".into(), json!(d.serial));
        if let Some((user, at)) = &d.user {
            if let Ok(u) = self.entity(Kind::User, user) {
                m.insert("User".into(), crate::view::entity(u, false));
            }
            m.insert("EnableDate".into(), secs(*at));
        }
        Value::Object(m)
    }

    fn device_mut(&mut self, serial: &str) -> Result<&mut Mfa, ApiError> {
        self.held
            .mfa
            .get_mut(serial)
            .ok_or_else(|| no_such("MFA device", serial))
    }

    pub(crate) fn tag_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        let d = self.device_mut(r.s("SerialNumber"))?;
        let added = new.iter().filter(|(k, _)| !d.tags.contains_key(k)).count();
        if d.tags.len() + added > MAX_TAGS {
            return Err(limit("Cannot exceed quota for TagsPerMFADevice: 50"));
        }
        d.tags.extend(new);
        Ok(Value::Null)
    }

    pub(crate) fn untag_mfa_device(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys = r.strings("TagKeys");
        let d = self.device_mut(r.s("SerialNumber"))?;
        for k in keys {
            d.tags.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_mfa_device_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let serial = r.s("SerialNumber");
        let d = self
            .held
            .mfa
            .get(serial)
            .ok_or_else(|| no_such("MFA device", serial))?;
        let Value::Array(items) = tag_list(d.tags.iter()) else {
            return Ok(Value::Null);
        };
        page(r, items, "Tags")
    }
}

/// What STS says when an MFA code does not open a session (memory: AWS's two sentences).
pub(crate) enum MfaFault {
    /// No device by that serial is enabled for the caller.
    Unknown,
    /// The code is not the device's code now, or was used already.
    Wrong,
}

impl MfaFault {
    pub(crate) fn refusal(&self) -> ApiError {
        let message = match self {
            Self::Unknown => {
                "MultiFactorAuthentication failed, unable to validate MFA code. Please verify your MFA serial number is valid and associated with this user."
            }
            Self::Wrong => "MultiFactorAuthentication failed with invalid MFA one time pass code. ",
        };
        ApiError::new(403, "AccessDenied", message)
    }
}

impl Core {
    /// The step of the code `token` from device `serial` enabled for the caller (`user`, by name), if it is the device's code now
    /// (one step either way) and has not been used.
    pub(crate) fn mfa_step(
        &self,
        user: Option<&str>,
        serial: &str,
        token: &str,
    ) -> Result<i64, MfaFault> {
        let d = user
            .and_then(|u| self.users_device(u, serial).ok())
            .ok_or(MfaFault::Unknown)?;
        let now = totp::step_of(self.now_ms());
        totp::matching(&d.seed, (now - 1)..=(now + 1), token)
            .filter(|s| *s > d.last_step)
            .ok_or(MfaFault::Wrong)
    }

    /// Checks a session request's MFA code and spends it. `Ok(None)` when the request names no device.
    pub(crate) fn spend_mfa(
        &mut self,
        r: &Req<'_>,
        user: Option<&str>,
    ) -> Result<Option<i64>, ApiError> {
        let Some(serial) = r.opt("SerialNumber") else {
            return Ok(None);
        };
        let token = r.s("TokenCode");
        let step = self
            .mfa_step(user, serial, token)
            .map_err(|f| f.refusal())?;
        if let Some(d) = self.held.mfa.get_mut(serial) {
            d.last_step = step;
        }
        Ok(Some(self.now_ms()))
    }
}
