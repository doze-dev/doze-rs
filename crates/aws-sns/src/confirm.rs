//! The `SubscribeURL` an `http` endpoint is sent (`webhook.rs`): an unsigned `ConfirmSubscription` by `TopicArn` and `Token`, as AWS's
//! is, answered in the Query protocol's XML.

use aws_model_sns::Operation;
use aws_model_sns::tables::TABLES;
use aws_wire::service::{Call, Caller, WebReply, WebRequest};
use aws_wire::{ApiError, RequestId};
use serde_json::{Value, json};

use crate::Sns;

/// The query string's pairs, decoded.
fn pairs(query: &str) -> Vec<(String, String)> {
    let dec = |s: &str| {
        let s = s.replace('+', " ");
        aws_core::uri::decode_str(&s).map_or(s.clone(), std::borrow::Cow::into_owned)
    };
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (dec(k), dec(v))
        })
        .collect()
}

/// The `ConfirmSubscription` call a `SubscribeURL` request makes.
fn call_of(req: &WebRequest, id: &RequestId) -> Result<Call, ApiError> {
    let query = pairs(&req.query);
    let get = |k: &str| {
        query
            .iter()
            .find(|(n, _)| n == k)
            .map_or("", |(_, v)| v.as_str())
    };
    if get("Action") != "ConfirmSubscription" {
        return Err(ApiError::new(
            400,
            "InvalidAction",
            "Only ConfirmSubscription is served here.",
        ));
    }
    let topic = get("TopicArn");
    let parts: Vec<&str> = topic.splitn(6, ':').collect();
    let (Some(region), Some(account)) = (parts.get(3), parts.get(4)) else {
        return Err(ApiError::new(
            400,
            "InvalidParameter",
            "Invalid parameter: TopicArn",
        ));
    };
    Ok(Call {
        op: Operation::ConfirmSubscription.index(),
        id: id.clone(),
        caller: Caller {
            access_key_id: Caller::ANONYMOUS.to_owned(),
            account_id: (*account).to_owned(),
            region: (*region).to_owned(),
            endpoint: req.endpoint.clone(),
            service: None,
            session_token: None,
        },
        body: json!({"TopicArn": topic, "Token": get("Token")}).to_string(),
        arena: aws_schema::arena::Arena::new(),
        root: aws_schema::arena::NIL,
    })
}

impl Sns {
    pub(crate) async fn confirm_by_url(&self, req: WebRequest) -> WebReply {
        let id = RequestId::new();
        let call = match call_of(&req, &id) {
            Ok(call) => call,
            Err(e) => return xml_reply(&id, Err(e)),
        };
        let answered = self.ask(call).await.and_then(|out| {
            let shape = TABLES
                .op(Operation::ConfirmSubscription.index())
                .and_then(|o| o.output)
                .ok_or_else(|| ApiError::internal("no output shape"))?;
            let mut text = Vec::new();
            aws_schema::enc::encode(&TABLES, shape, &out.arena, "", out.root, &mut text)
                .map_err(|_| ApiError::internal("the answer could not be written"))?;
            let v: Value = serde_json::from_slice(&text).unwrap_or_default();
            Ok(v.get("SubscriptionArn")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned())
        });
        xml_reply(&id, answered)
    }
}

/// The Query protocol's answer to a `ConfirmSubscription` (the shape is the protocol's; the namespace is SNS's).
fn xml_reply(id: &RequestId, answer: Result<String, ApiError>) -> WebReply {
    let ns = "http://sns.amazonaws.com/doc/2010-03-31/";
    let (status, body) = match answer {
        Ok(arn) => (
            200,
            format!(
                "<ConfirmSubscriptionResponse xmlns=\"{ns}\"><ConfirmSubscriptionResult><SubscriptionArn>{arn}</SubscriptionArn></ConfirmSubscriptionResult><ResponseMetadata><RequestId>{}</RequestId></ResponseMetadata></ConfirmSubscriptionResponse>",
                id.as_str()
            ),
        ),
        Err(e) => (
            e.status,
            format!(
                "<ErrorResponse xmlns=\"{ns}\"><Error><Type>Sender</Type><Code>{}</Code><Message>{}</Message></Error><RequestId>{}</RequestId></ErrorResponse>",
                e.code,
                e.message.replace('&', "&amp;").replace('<', "&lt;"),
                id.as_str()
            ),
        ),
    };
    WebReply {
        status,
        headers: vec![("content-type".to_owned(), "text/xml".to_owned())],
        body: bytes::Bytes::from(body),
    }
}
