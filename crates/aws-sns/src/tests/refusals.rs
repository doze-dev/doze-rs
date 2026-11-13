//! Authorization: what each request asks to do, and how a refusal is worded.

use aws_peers::{Authorizer, Need, Peers};
use aws_wire::ApiError;
use aws_wire::service::{BoxFuture, Caller};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

use super::{ACCOUNT, World, arn};

/// An authorizer that writes down what it was asked, and refuses what a test tells it to.
#[derive(Default)]
struct Spy {
    asked: Mutex<Vec<Need>>,
    refuse: bool,
}

impl Authorizer for Spy {
    fn authorize<'a>(
        &'a self,
        _: &'a Caller,
        needs: Vec<Need>,
    ) -> BoxFuture<'a, Result<(), ApiError>> {
        let refuse = self.refuse;
        if let Ok(mut asked) = self.asked.lock() {
            asked.extend(needs.clone());
        }
        Box::pin(async move {
            if refuse {
                let n = needs
                    .first()
                    .map(|n| (n.action.clone(), n.resource.clone()))
                    .unwrap_or_default();
                Err(ApiError::new(
                    400,
                    "AccessDeniedException",
                    format!(
                        "User: me is not authorized to perform: {} on resource: {}",
                        n.0, n.1
                    ),
                ))
            } else {
                Ok(())
            }
        })
    }
}

/// Runs `calls` with the authorizer in force; returns what was asked, in order.
fn asked(spy: &Arc<Spy>, w: &mut World, calls: &[(&str, Value)]) -> Vec<Need> {
    let peers: &Arc<Peers> = &w.peers;
    peers.set_authorizer(spy.clone());
    for (op, request) in calls {
        let _ = w.handled(op, request);
    }
    let out = spy.asked.lock().unwrap().clone();
    spy.asked.lock().unwrap().clear();
    out
}

fn wanted(n: &Need) -> (&str, &str) {
    (&n.action, &n.resource)
}

#[test]
fn each_request_asks_for_the_action_aws_words_in_a_refusal_on_the_arn_it_acts_on() {
    let mut w = World::new();
    let topic = w.topic("t");
    let spy = Arc::new(Spy::default());
    let q = "arn:aws:sqs:us-east-1:000000000000:q";
    let sub = w.subscribe_queue(&topic, q, &json!({}));
    let needs = asked(
        &spy,
        &mut w,
        &[
            (
                "CreateTopic",
                json!({"Name": "new", "Tags": [{"Key": "a", "Value": "b"}]}),
            ),
            ("Publish", json!({"TopicArn": topic, "Message": "x"})),
            (
                "PublishBatch",
                json!({"TopicArn": topic, "PublishBatchRequestEntries": [{"Id": "a", "Message": "x"}]}),
            ),
            (
                "Publish",
                json!({"PhoneNumber": "+14155552671", "Message": "x"}),
            ),
            ("Unsubscribe", json!({"SubscriptionArn": sub})),
            ("ListTopics", json!({})),
            (
                "TagResource",
                json!({"ResourceArn": topic, "Tags": [{"Key": "a", "Value": "b"}]}),
            ),
            (
                "CreatePlatformApplication",
                json!({"Name": "app", "Platform": "GCM", "Attributes": {"PlatformCredential": "k"}}),
            ),
        ],
    );
    let got: Vec<(&str, &str)> = needs.iter().map(wanted).collect();
    assert_eq!(
        got,
        [
            ("SNS:CreateTopic", arn("new").as_str()),
            ("SNS:TagResource", arn("new").as_str()),
            ("SNS:Publish", topic.as_str()),
            ("SNS:Publish", topic.as_str()),
            ("SNS:Publish", "*"),
            ("SNS:Unsubscribe", topic.as_str()),
            ("SNS:ListTopics", "*"),
            ("SNS:TagResource", topic.as_str()),
            (
                "SNS:CreatePlatformApplication",
                "arn:aws:sns:us-east-1:000000000000:app/GCM/app"
            ),
        ]
    );
}

#[test]
fn a_topic_brings_its_policy_and_the_owner_the_default_policy_hangs_on() {
    let mut w = World::new();
    let topic = w.topic("t");
    let spy = Arc::new(Spy::default());
    let needs = asked(
        &spy,
        &mut w,
        &[("GetTopicAttributes", json!({"TopicArn": topic}))],
    );
    let need = needs.first().unwrap();
    assert_eq!(
        need.context,
        vec![("AWS:SourceOwner".to_owned(), vec![ACCOUNT.to_owned()])]
    );
    let policy: Value = serde_json::from_str(need.policy.as_ref().unwrap()).unwrap();
    assert_eq!(policy["Id"], "__default_policy_ID");
    // A topic that is not there brings no policy; the request is refused on its own terms.
    let absent = asked(
        &spy,
        &mut w,
        &[("GetTopicAttributes", json!({"TopicArn": arn("absent")}))],
    );
    assert!(absent.first().unwrap().policy.is_none());
}

#[test]
fn a_refusal_is_the_authorization_error_in_the_words_iam_gives() {
    let mut w = World::new();
    let spy = Arc::new(Spy {
        refuse: true,
        ..Spy::default()
    });
    w.peers.set_authorizer(spy);
    w.peers.set_mode(aws_peers::Mode::Enforce);
    let e = w.handled("CreateTopic", &json!({"Name": "u"})).unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("AuthorizationErrorException"));
    assert_eq!(
        e.message,
        "User: me is not authorized to perform: SNS:CreateTopic on resource: arn:aws:sns:us-east-1:000000000000:u"
    );
    assert_eq!(
        w.core.topics.len(),
        0,
        "the refused request changed nothing"
    );
    // Nothing is asked when the mode is off.
    w.peers.set_mode(aws_peers::Mode::Off);
    w.handled("CreateTopic", &json!({"Name": "u"})).unwrap();
    assert_eq!(w.core.topics.len(), 1);
}
