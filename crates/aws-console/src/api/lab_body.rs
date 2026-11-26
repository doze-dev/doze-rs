//! What Send a call does with an operation that carries an object's body: S3 `PutObject`.
//!
//! The peer bus carries a payload (the console's own upload sends one, `deep::upload`), so `PutObject` is sent the way an upload is: through
//! `Bus::call_rest`, with the bytes. The Lab's input is JSON, so the body is the input's `Body` member, **text**, at most [`BODY_MAX`]
//! (64 KiB): the Lab is for small calls, and the bucket's page uploads files. `UploadPart` is one part of a multipart upload (it needs an
//! upload ID from `CreateMultipartUpload`, and every part but the last is at least 5 MiB), so it stays refused, with that reason.
//!
//! Like every Send a call, this is the console's own call: it goes over the bus and not through the gateway, so no Lab rule sees it.

use axum::body::Bytes;
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use super::error;

/// The most a Send a call's body may be: 64 KiB.
pub(super) const BODY_MAX: usize = 64 << 10;

/// Why `UploadPart` is not sent from the Lab.
const PART_WHY: &str = "UploadPart sends one part of a multipart upload, which needs the upload ID CreateMultipartUpload gave, and every part but the last is at least 5 MiB, so a Send a call cannot make one. Send PutObject for a small object, or use the AWS CLI for a multipart upload.";

/// The most text the operation's `Body` may be, for an operation that carries an object's body.
#[must_use]
pub(super) fn body_max(target: &str, operation: &str) -> Option<u32> {
    (target == "s3" && operation == "PutObject")
        .then(|| u32::try_from(BODY_MAX).unwrap_or(u32::MAX))
}

/// Why the Lab does not send the operation, when it does not.
#[must_use]
pub(super) fn cannot(target: &str, operation: &str) -> Option<String> {
    (target == "s3" && operation == "UploadPart").then(|| PART_WHY.to_owned())
}

/// The call's input without its body, and the body, for the operations that carry one (`None` for any other). The body is empty when the
/// input has none, as `PutObject` of nothing makes an empty object on AWS.
///
/// # Errors
/// The answer that says why the call is refused: `UploadPart`, a body that is not text, or a body past [`BODY_MAX`].
pub(super) fn split(
    target: &str,
    operation: &str,
    mut input: Value,
) -> Result<(Value, Option<Bytes>), Box<Response>> {
    if target != "s3" {
        return Ok((input, None));
    }
    if operation == "UploadPart" {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "NEEDS_MULTIPART",
            PART_WHY,
        )));
    }
    if operation != "PutObject" {
        return Ok((input, None));
    }
    let taken = input.as_object_mut().and_then(|o| o.remove("Body"));
    let body = match taken {
        None | Some(Value::Null) => Bytes::new(),
        Some(Value::String(text)) if text.len() <= BODY_MAX => Bytes::from(text),
        Some(Value::String(text)) => {
            return Err(Box::new(error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "BODY_TOO_LARGE",
                &format!(
                    "The body is {} bytes: a Send a call carries at most {BODY_MAX} (64 KiB). Upload larger objects on the bucket's page, or with the AWS CLI.",
                    text.len()
                ),
            )));
        }
        Some(_) => {
            return Err(Box::new(error(
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "The body is text: Body is a JSON string.",
            )));
        }
    };
    if let Some(o) = input.as_object_mut() {
        o.insert("ContentLength".to_owned(), Value::from(body.len() as u64));
    }
    Ok((input, Some(body)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn put(input: Value) -> Result<(Value, Option<Bytes>), Box<Response>> {
        split("s3", "PutObject", input)
    }

    #[test]
    fn a_put_takes_its_body_out_of_the_input_and_says_its_length() {
        let (input, body) = put(json!({ "Bucket": "b", "Key": "k", "Body": "héllo" })).unwrap();
        assert_eq!(body.as_deref(), Some("héllo".as_bytes()));
        assert_eq!(
            input,
            json!({ "Bucket": "b", "Key": "k", "ContentLength": 6 })
        );
        // A length the form sent is not believed.
        let (input, _) =
            put(json!({ "Bucket": "b", "Key": "k", "Body": "ab", "ContentLength": 99 })).unwrap();
        assert_eq!(input["ContentLength"], 2);
    }

    #[test]
    fn no_body_is_an_empty_object() {
        for input in [
            json!({ "Bucket": "b", "Key": "k" }),
            json!({ "Bucket": "b", "Key": "k", "Body": null }),
        ] {
            let (input, body) = put(input).unwrap();
            assert_eq!(body.as_deref(), Some(&b""[..]));
            assert_eq!(input["ContentLength"], 0);
        }
    }

    #[test]
    fn a_body_is_text_and_at_most_64_kib() {
        let at_most = "a".repeat(BODY_MAX);
        assert_eq!(
            put(json!({ "Body": at_most })).unwrap().1.unwrap().len(),
            BODY_MAX
        );
        let past = "a".repeat(BODY_MAX + 1);
        let refused = put(json!({ "Body": past })).unwrap_err();
        assert_eq!(refused.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let not_text = put(json!({ "Body": [1, 2] })).unwrap_err();
        assert_eq!(not_text.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn an_upload_part_is_refused_with_its_reason_and_other_calls_are_not_touched() {
        let refused = split("s3", "UploadPart", json!({ "Body": "x" })).unwrap_err();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let input = json!({ "Bucket": "b", "Body": "x" });
        assert_eq!(
            split("s3", "GetObject", input.clone()).unwrap(),
            (input.clone(), None)
        );
        // Another service's `Body` member is its own.
        assert_eq!(
            split("sqs", "PutObject", input.clone()).unwrap(),
            (input, None)
        );
    }
}
