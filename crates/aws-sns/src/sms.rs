//! SMS: the region's settings, and the opt-out operations. **Nothing is sent** (decision D1): a text message published to a phone number
//! is accepted and gets a message id, and that is all; no number is ever opted out, because only a recipient can opt out. The wording of
//! the refusals is from memory.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::{Core, SMS_KEY};
use crate::model::Sms;
use crate::req::{Req, invalid, invalid_because};

/// The settings SNS has for SMS.
const SETTINGS: &[&str] = &[
    "MonthlySpendLimit",
    "DeliveryStatusIAMRole",
    "DeliveryStatusSuccessSamplingRate",
    "DefaultSenderID",
    "DefaultSMSType",
    "UsageReportS3Bucket",
];

/// Whether `text` is a phone number in the E.164 form: `+` and 7 to 15 digits.
pub(crate) fn is_phone_number(text: &str) -> bool {
    text.strip_prefix('+')
        .is_some_and(|d| (7..=15).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

fn check_setting(name: &str, value: &str) -> Result<(), ApiError> {
    let bad = |why: &str| Err(invalid_because("attributes", format!("{name}: {why}")));
    match name {
        "DefaultSMSType" if !matches!(value, "Promotional" | "Transactional") => {
            bad("must be Promotional or Transactional")
        }
        "DeliveryStatusSuccessSamplingRate" if value.parse::<u32>().is_ok_and(|n| n <= 100) => {
            Ok(())
        }
        "DeliveryStatusSuccessSamplingRate" => bad("must be between 0 and 100"),
        "MonthlySpendLimit" if value.parse::<u64>().is_err() => bad("must be a whole number"),
        "DefaultSenderID" if !is_sender_id(value) => {
            bad("must be 1 to 11 letters and digits, with at least one letter")
        }
        other if SETTINGS.contains(&other) => Ok(()),
        other => Err(invalid_because(
            "attributes",
            format!("Invalid attribute: {other}"),
        )),
    }
}

fn is_sender_id(value: &str) -> bool {
    (1..=11).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_alphanumeric())
        && value.bytes().any(|b| b.is_ascii_alphabetic())
}

impl Core {
    fn sms_settings(&self) -> BTreeMap<String, String> {
        self.sms
            .get(SMS_KEY)
            .map(|s| s.attrs.clone())
            .unwrap_or_default()
    }

    /// `SetSMSAttributes`.
    pub(crate) fn set_sms_attributes(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.map("attributes");
        for (k, v) in &given {
            check_setting(k, v)?;
        }
        let mut next = self.sms_settings();
        next.extend(given);
        self.sms.insert(SMS_KEY.to_owned(), Sms { attrs: next });
        Ok(Value::Null)
    }

    /// `GetSMSAttributes`: those asked for, or all that were set.
    pub(crate) fn get_sms_attributes(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let wanted = r.strings("attributes");
        let attrs: BTreeMap<String, String> = self
            .sms_settings()
            .into_iter()
            .filter(|(k, _)| wanted.is_empty() || wanted.contains(k))
            .collect();
        Ok(json!({"attributes": attrs}))
    }

    /// `CheckIfPhoneNumberIsOptedOut`: no number is.
    pub(crate) fn check_opted_out(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        check_number(r.s("phoneNumber"))?;
        Ok(json!({"isOptedOut": false}))
    }

    /// `OptInPhoneNumber`: a number that was never opted out is already in.
    pub(crate) fn opt_in(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        check_number(r.s("phoneNumber"))?;
        Ok(Value::Null)
    }

    /// `ListPhoneNumbersOptedOut`: none.
    pub(crate) fn list_opted_out(&self) -> Value {
        json!({"phoneNumbers": []})
    }
}

/// The refusal for a number that is not E.164, as `CheckIfPhoneNumberIsOptedOut` and `OptInPhoneNumber` word it (memory).
fn check_number(number: &str) -> Result<(), ApiError> {
    if is_phone_number(number) {
        Ok(())
    } else {
        Err(invalid("phoneNumber"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_numbers_and_settings_are_checked() {
        assert!(is_phone_number("+14155552671"));
        assert!(!is_phone_number("14155552671"));
        assert!(!is_phone_number("+1415"));
        assert!(!is_phone_number("+1415555267x"));
        assert!(check_setting("DefaultSMSType", "Transactional").is_ok());
        assert!(check_setting("DefaultSMSType", "Both").is_err());
        assert!(check_setting("DefaultSenderID", "12345").is_err());
        assert!(check_setting("DefaultSenderID", "Doze1").is_ok());
        assert!(check_setting("DeliveryStatusSuccessSamplingRate", "101").is_err());
        assert!(check_setting("Colour", "red").is_err());
    }
}
