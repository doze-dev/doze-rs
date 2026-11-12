//! State on disk: after every call of a broad scenario the service is stopped and started again, and what it loads is exactly
//! what it had.

use serde_json::{Value, json};

use super::{START, World, temp};
use crate::core::Core;

struct Run<'a> {
    w: World,
    dir: &'a std::path::Path,
}

impl<'a> Run<'a> {
    fn new(dir: &'a std::path::Path) -> Self {
        let clock = std::sync::Arc::new(doze_runtime::ManualClock::at(START));
        let base = World::new();
        let core = Core::open_in(dir, clock.clone(), Some(base.peers.clone())).unwrap();
        Self {
            w: World::with(core, clock, base.peers.clone(), temp()),
            dir,
        }
    }

    fn state(&self) -> String {
        let c = &self.w.core;
        format!(
            "{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
            c.topics, c.subs, c.apps, c.endpoints, c.sms
        )
    }

    fn step(&mut self, op: &str, request: &Value) -> Value {
        let answer = self.w.ok(op, request);
        let before = self.state();
        if let Some(db) = self.w.core.db.take() {
            db.close();
        }
        self.w.core =
            Core::open_in(self.dir, self.w.clock.clone(), Some(self.w.peers.clone())).unwrap();
        assert_eq!(before, self.state(), "after {op} {request}");
        answer
    }
}

#[test]
fn a_restart_changes_nothing_whatever_was_done_before_it() {
    let dir = temp();
    let mut r = Run::new(dir.path());
    let t = r.step("CreateTopic", &json!({"Name": "t", "Attributes": {"DisplayName": "d"}, "Tags": [{"Key": "k", "Value": "v"}]}));
    let topic = t["TopicArn"].as_str().unwrap().to_owned();
    let f = r.step("CreateTopic", &json!({"Name": "f.fifo", "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true"}}));
    let fifo = f["TopicArn"].as_str().unwrap().to_owned();
    r.step("SetTopicAttributes", &json!({"TopicArn": topic, "AttributeName": "MaximumMessageSize", "AttributeValue": "4096"}));
    r.step("AddPermission", &json!({"TopicArn": topic, "Label": "l", "AWSAccountId": ["111122223333"], "ActionName": ["Publish"]}));
    r.step(
        "TagResource",
        &json!({"ResourceArn": topic, "Tags": [{"Key": "a", "Value": "b"}]}),
    );
    r.step(
        "PutDataProtectionPolicy",
        &json!({"ResourceArn": topic, "DataProtectionPolicy": "{}"}),
    );
    after_subscribing(&mut r, &topic, &fifo);
}

fn after_subscribing(r: &mut Run<'_>, topic: &str, fifo: &str) {
    let s = r.step("Subscribe", &json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q", "Attributes": {"FilterPolicy": "{\"a\": [1]}"}}));
    let sub = s["SubscriptionArn"].as_str().unwrap().to_owned();
    r.step("SetSubscriptionAttributes", &json!({"SubscriptionArn": sub, "AttributeName": "RawMessageDelivery", "AttributeValue": "true"}));
    r.step(
        "Subscribe",
        &json!({"TopicArn": topic, "Protocol": "email", "Endpoint": "a@example.com"}),
    );
    r.step(
        "Publish",
        &json!({"TopicArn": fifo, "Message": "x", "MessageGroupId": "g"}),
    );
    r.step(
        "Publish",
        &json!({"TopicArn": fifo, "Message": "x", "MessageGroupId": "g"}),
    );
    r.step(
        "PublishBatch",
        &json!({"TopicArn": topic, "PublishBatchRequestEntries": [{"Id": "a", "Message": "m"}]}),
    );
    let app = r.step(
        "CreatePlatformApplication",
        &json!({"Name": "app", "Platform": "GCM", "Attributes": {"PlatformCredential": "k"}}),
    );
    let endpoint = r.step(
        "CreatePlatformEndpoint",
        &json!({"PlatformApplicationArn": app["PlatformApplicationArn"], "Token": "tok"}),
    );
    r.step(
        "SetEndpointAttributes",
        &json!({"EndpointArn": endpoint["EndpointArn"], "Attributes": {"Enabled": "false"}}),
    );
    r.step(
        "SetSMSAttributes",
        &json!({"attributes": {"DefaultSMSType": "Promotional"}}),
    );
    r.step("Unsubscribe", &json!({"SubscriptionArn": sub}));
    r.step(
        "RemovePermission",
        &json!({"TopicArn": topic, "Label": "l"}),
    );
    r.step(
        "UntagResource",
        &json!({"ResourceArn": topic, "TagKeys": ["a"]}),
    );
    r.step(
        "DeleteEndpoint",
        &json!({"EndpointArn": endpoint["EndpointArn"]}),
    );
    r.step(
        "DeletePlatformApplication",
        &json!({"PlatformApplicationArn": app["PlatformApplicationArn"]}),
    );
    r.step("DeleteTopic", &json!({"TopicArn": topic}));
    // What was loaded still answers.
    assert_eq!(
        r.w.ok("ListTopics", &json!({}))["Topics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_call_writes_the_rows_it_changed_and_a_read_writes_none() {
    let mut w = World::new();
    let rows = |w: &mut World, op: &str, request: &Value| w.rows_of(op, request);
    assert_eq!(rows(&mut w, "CreateTopic", &json!({"Name": "t"})), 1);
    let topic = super::arn("t");
    assert_eq!(
        rows(&mut w, "GetTopicAttributes", &json!({"TopicArn": topic})),
        0
    );
    assert_eq!(rows(&mut w, "ListTopics", &json!({})), 0);
    assert_eq!(
        rows(
            &mut w,
            "Publish",
            &json!({"TopicArn": topic, "Message": "x"})
        ),
        0
    );
    assert_eq!(
        rows(
            &mut w,
            "SetTopicAttributes",
            &json!({"TopicArn": topic, "AttributeName": "DisplayName", "AttributeValue": "d"})
        ),
        1
    );
    let sub = json!({"TopicArn": topic, "Protocol": "sqs", "Endpoint": "arn:aws:sqs:us-east-1:000000000000:q"});
    assert_eq!(rows(&mut w, "Subscribe", &sub), 1);
    assert_eq!(
        rows(&mut w, "Subscribe", &sub),
        0,
        "the same subscription again changes nothing"
    );
}
