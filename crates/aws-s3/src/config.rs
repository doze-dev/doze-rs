//! Bucket configuration: CORS, policy, lifecycle, encryption, website, logging, notification, ownership, public access block,
//! accelerate, request payment, replication and Object Lock. Each is stored as the request carried it and returned as sent (so a
//! template or a Terraform refresh sees no drift), after the checks AWS makes of it. Only the policy, the lifecycle rules, the
//! encryption default and the notification targets act on anything locally.

use aws_model_s3::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::errors::{malformed_xml, no_config};
use crate::req::{Answer, Out, Req};
use crate::validate::{check, defaults};

/// One kind of configuration and the operations that read and write it.
pub(crate) struct Kind {
    pub(crate) name: &'static str,
    pub(crate) put: Operation,
    /// The input member that holds the configuration.
    pub(crate) member: &'static str,
    pub(crate) get: Operation,
    /// The output member that holds it, or `None` when the output's own members are the configuration.
    pub(crate) under: Option<&'static str>,
    pub(crate) delete: Option<Operation>,
}

use Operation as O;

const fn kind(
    name: &'static str,
    put: Operation,
    member: &'static str,
    get: Operation,
    under: Option<&'static str>,
    delete: Option<Operation>,
) -> Kind {
    Kind { name, put, member, get, under, delete }
}

pub(crate) const KINDS: &[Kind] = &[
    kind("cors", O::PutBucketCors, "CORSConfiguration", O::GetBucketCors, None, Some(O::DeleteBucketCors)),
    kind("policy", O::PutBucketPolicy, "Policy", O::GetBucketPolicy, Some("Policy"), Some(O::DeleteBucketPolicy)),
    kind("lifecycle", O::PutBucketLifecycleConfiguration, "LifecycleConfiguration", O::GetBucketLifecycleConfiguration, None, Some(O::DeleteBucketLifecycle)),
    kind("encryption", O::PutBucketEncryption, "ServerSideEncryptionConfiguration", O::GetBucketEncryption, Some("ServerSideEncryptionConfiguration"), Some(O::DeleteBucketEncryption)),
    kind("website", O::PutBucketWebsite, "WebsiteConfiguration", O::GetBucketWebsite, None, Some(O::DeleteBucketWebsite)),
    kind("logging", O::PutBucketLogging, "BucketLoggingStatus", O::GetBucketLogging, None, None),
    kind("notification", O::PutBucketNotificationConfiguration, "NotificationConfiguration", O::GetBucketNotificationConfiguration, None, None),
    kind("ownership", O::PutBucketOwnershipControls, "OwnershipControls", O::GetBucketOwnershipControls, Some("OwnershipControls"), Some(O::DeleteBucketOwnershipControls)),
    kind("public-access-block", O::PutPublicAccessBlock, "PublicAccessBlockConfiguration", O::GetPublicAccessBlock, Some("PublicAccessBlockConfiguration"), Some(O::DeletePublicAccessBlock)),
    kind("accelerate", O::PutBucketAccelerateConfiguration, "AccelerateConfiguration", O::GetBucketAccelerateConfiguration, None, None),
    kind("request-payment", O::PutBucketRequestPayment, "RequestPaymentConfiguration", O::GetBucketRequestPayment, None, None),
    kind("replication", O::PutBucketReplication, "ReplicationConfiguration", O::GetBucketReplication, Some("ReplicationConfiguration"), Some(O::DeleteBucketReplication)),
    kind("object-lock", O::PutObjectLockConfiguration, "ObjectLockConfiguration", O::GetObjectLockConfiguration, Some("ObjectLockConfiguration"), None),
];

/// The kind of configuration operation `op` reads, writes or removes.
pub(crate) fn kind_of(op: Operation) -> Option<&'static Kind> {
    KINDS
        .iter()
        .find(|k| k.put == op || k.get == op || k.delete == Some(op))
}

impl Core {
    pub(crate) async fn configuration(&mut self, req: &Req<'_>, op: Operation) -> Answer {
        let Some(k) = kind_of(op) else {
            return Err(ApiError::new(501, "NotImplemented", "This operation is not built in doze-aws."));
        };
        self.require(req.bucket())?;
        if op == k.put {
            self.put_config(req, k).await
        } else if op == k.get {
            self.get_config(req, k)
        } else {
            self.delete_config(req, k)
        }
    }

    async fn put_config(&mut self, req: &Req<'_>, k: &Kind) -> Answer {
        let body = req.get(k.member).ok_or_else(malformed_xml)?;
        let mut stored = body.clone();
        check(self, req, k.name, &mut stored)?;
        if k.name == "notification" {
            self.check_notification(req, &mut stored).await?;
        }
        let text = match &stored {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if let Some(b) = self.buckets.get_mut(req.bucket()) {
            b.configs.insert(k.name.to_owned(), text);
        }
        if k.name == "lifecycle" {
            return Out::json(json!({"TransitionDefaultMinimumObjectSize": "all_storage_classes_128K"}));
        }
        if k.name == "policy" {
            return Out::no_content();
        }
        Out::empty()
    }

    fn get_config(&self, req: &Req<'_>, k: &Kind) -> Answer {
        let stored = self.require(req.bucket())?.configs.get(k.name);
        let value = match stored {
            Some(text) if k.name == "policy" => Value::String(text.clone()),
            Some(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            None => defaults(k.name).ok_or_else(|| no_config(k.name))?,
        };
        let mut out = match k.under {
            Some(member) => {
                let mut m = Map::new();
                m.insert(member.to_owned(), value);
                m
            }
            None => value.as_object().cloned().unwrap_or_default(),
        };
        if k.name == "lifecycle" {
            out.insert("TransitionDefaultMinimumObjectSize".into(), json!("all_storage_classes_128K"));
        }
        Out::json(Value::Object(out))
    }

    fn delete_config(&mut self, req: &Req<'_>, k: &Kind) -> Answer {
        if let Some(b) = self.buckets.get_mut(req.bucket()) {
            b.configs.remove(k.name);
        }
        Out::empty()
    }
}
