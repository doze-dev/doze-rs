//! The JSON an SQS queue receives for a notification when the subscription is not raw (the fields and their order recorded 2026-11-09).
//!
//! **The signature is not one.** SCOPE.md keeps the envelope's fields and cuts message signing: a validator that wants SNS's certificate
//! cannot be satisfied locally. `Signature` is a digest of the text AWS would sign, `SigningCertURL` names a certificate this endpoint
//! does not serve, and `UnsubscribeURL` is the local endpoint's.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::message::{Attr, Message};

#[derive(Serialize)]
struct EnvelopeAttribute<'a> {
    #[serde(rename = "Type")]
    kind: &'a str,
    #[serde(rename = "Value")]
    value: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Notification<'a> {
    #[serde(rename = "Type")]
    kind: &'a str,
    message_id: &'a str,
    topic_arn: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<&'a str>,
    message: &'a str,
    timestamp: &'a str,
    signature_version: &'a str,
    signature: &'a str,
    #[serde(rename = "SigningCertURL")]
    signing_cert_url: &'a str,
    #[serde(rename = "UnsubscribeURL")]
    unsubscribe_url: &'a str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    message_attributes: BTreeMap<&'a str, EnvelopeAttribute<'a>>,
}

/// Where a notification is from and which subscription it is for.
pub(crate) struct Context<'a> {
    pub(crate) message_id: &'a str,
    pub(crate) topic_arn: &'a str,
    pub(crate) subscription_arn: &'a str,
    pub(crate) timestamp: &'a str,
    /// The scheme and host the caller used.
    pub(crate) endpoint: &'a str,
}

/// The notification for one subscription, with `text` as its message.
pub(crate) fn notification(ctx: &Context<'_>, m: &Message, text: &str) -> String {
    let to_sign = string_to_sign(ctx, m, text);
    let signature = aws_core::base64::encode(&Sha256::digest(to_sign.as_bytes()));
    let cert = format!("{}/SimpleNotificationService-doze.pem", ctx.endpoint);
    let unsubscribe = format!(
        "{}/?Action=Unsubscribe&SubscriptionArn={}",
        ctx.endpoint, ctx.subscription_arn
    );
    let note = Notification {
        kind: "Notification",
        message_id: ctx.message_id,
        topic_arn: ctx.topic_arn,
        subject: m.subject.as_deref(),
        message: text,
        timestamp: ctx.timestamp,
        signature_version: "1",
        signature: &signature,
        signing_cert_url: &cert,
        unsubscribe_url: &unsubscribe,
        message_attributes: m
            .attrs
            .iter()
            .map(|(k, a)| (k.as_str(), shown(a)))
            .collect(),
    };
    serde_json::to_string(&note).unwrap_or_default()
}

/// The event a Lambda function receives for one notification: AWS's `Records[].Sns` shape. It differs from the queue envelope in
/// three ways (AWS documentation, (memory)): the URLs are spelt `SigningCertUrl` and `UnsubscribeUrl`, `Subject` is `null` when
/// there is none, and `MessageAttributes` is always present.
pub(crate) fn lambda_event(ctx: &Context<'_>, m: &Message, text: &str) -> String {
    let note: serde_json::Value =
        serde_json::from_str(&notification(ctx, m, text)).unwrap_or(serde_json::Value::Null);
    let mut sns = serde_json::Map::new();
    let field = |k: &str| note.get(k).cloned().unwrap_or(serde_json::Value::Null);
    for k in [
        "Type",
        "MessageId",
        "TopicArn",
        "Subject",
        "Message",
        "Timestamp",
        "SignatureVersion",
        "Signature",
    ] {
        sns.insert(k.to_owned(), field(k));
    }
    sns.insert("SigningCertUrl".to_owned(), field("SigningCertURL"));
    sns.insert("UnsubscribeUrl".to_owned(), field("UnsubscribeURL"));
    sns.insert(
        "MessageAttributes".to_owned(),
        note.get("MessageAttributes")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())),
    );
    serde_json::json!({"Records": [{
        "EventSource": "aws:sns",
        "EventVersion": "1.0",
        "EventSubscriptionArn": ctx.subscription_arn,
        "Sns": sns,
    }]})
    .to_string()
}

fn shown(a: &Attr) -> EnvelopeAttribute<'_> {
    EnvelopeAttribute {
        kind: &a.data_type,
        value: a
            .string_value
            .as_deref()
            .or(a.binary_value.as_deref())
            .unwrap_or_default(),
    }
}

/// The text a notification's signature is made over, in AWS's order and form.
fn string_to_sign(ctx: &Context<'_>, m: &Message, text: &str) -> String {
    let mut out = format!("Message\n{text}\nMessageId\n{}\n", ctx.message_id);
    if let Some(s) = &m.subject {
        out.push_str(&format!("Subject\n{s}\n"));
    }
    out.push_str(&format!(
        "Timestamp\n{}\nTopicArn\n{}\nType\nNotification\n",
        ctx.timestamp, ctx.topic_arn
    ));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn the_fields_are_those_recorded_in_aws_order() {
        let mut m = Message {
            body: "shipped".into(),
            subject: Some("order".into()),
            ..Message::default()
        };
        m.attrs.insert(
            "kind".into(),
            Attr {
                data_type: "String".into(),
                string_value: Some("order".into()),
                binary_value: None,
            },
        );
        let ctx = Context {
            message_id: "id-1",
            topic_arn: "arn:aws:sns:us-east-1:1:t",
            subscription_arn: "arn:aws:sns:us-east-1:1:t:s",
            timestamp: "2026-11-12T10:00:00.123Z",
            endpoint: "http://127.0.0.1:4566",
        };
        let text = notification(&ctx, &m, "shipped");
        let keys: Vec<String> = serde_json::from_str::<serde_json::Map<String, Value>>(&text)
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let order: Vec<&str> = text
            .match_indices("\":")
            .filter_map(|(i, _)| text.get(..i)?.rsplit('"').next())
            .collect();
        assert!(
            order.starts_with(&["Type", "MessageId", "TopicArn", "Subject", "Message"]),
            "{order:?}"
        );
        assert_eq!(keys.len(), 11);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["MessageAttributes"]["kind"],
            serde_json::json!({"Type": "String", "Value": "order"})
        );
        assert_eq!(v["SignatureVersion"], "1");
        let no_subject = notification(&ctx, &Message::default(), "x");
        assert!(!no_subject.contains("Subject") && !no_subject.contains("MessageAttributes"));
    }
}
