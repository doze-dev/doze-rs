"""SNS through boto3, including the delivery into SQS a real stack depends on."""

import json

# What SNS signs a notification with. Present on both; equal on neither.
SIGNING = ("Timestamp", "Signature", "SigningCertURL", "UnsubscribeURL")


def subscribed_queue(client, names, cleanup, topic_arn, label="queue", **subscribe):
    """A queue that the topic is allowed to deliver to, and subscribed."""
    sqs, sns = client("sqs"), client("sns")
    url = sqs.create_queue(QueueName=names(label))["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "sqs:SendMessage", "Resource": arn,
            "Condition": {"ArnEquals": {"aws:SourceArn": topic_arn}},
        }],
    })})
    sub = sns.subscribe(TopicArn=topic_arn, Protocol="sqs", Endpoint=arn,
                        ReturnSubscriptionArn=True, **subscribe)
    cleanup(sns.unsubscribe, SubscriptionArn=sub["SubscriptionArn"])
    return url, sub


def one_body(client, eventually, url):
    sqs = client("sqs")

    def poll():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1)
        assert r.get("Messages"), "nothing delivered yet"
        return r["Messages"][0]["Body"]

    return eventually(poll)


def test_topic_lifecycle(client, names, cleanup, snapshot):
    sns = client("sns")
    name = names("topic")
    created = snapshot.match("create", sns.create_topic(Name=name))
    arn = created["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)

    snapshot.match("create-again", sns.create_topic(Name=name))
    attrs = sns.get_topic_attributes(TopicArn=arn)
    # The default policy is a JSON document inside a string; compare it as one.
    for doc in ("Policy", "EffectiveDeliveryPolicy"):
        if doc in attrs["Attributes"]:
            attrs["Attributes"][doc] = json.loads(attrs["Attributes"][doc])
    snapshot.match("attributes", attrs)

    snapshot.match("set-display-name", sns.set_topic_attributes(
        TopicArn=arn, AttributeName="DisplayName", AttributeValue="Orders"))
    snapshot.match("tag", sns.tag_resource(ResourceArn=arn, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("tags", sns.list_tags_for_resource(ResourceArn=arn))
    snapshot.match("delete", sns.delete_topic(TopicArn=arn))
    snapshot.match("delete-again", sns.delete_topic(TopicArn=arn))


def test_publish_reaches_a_queue_in_the_envelope(client, names, cleanup, snapshot, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url, sub = subscribed_queue(client, names, cleanup, arn)
    snapshot.match("subscribe", sub)
    snapshot.match("subscription-attributes",
                   sns.get_subscription_attributes(SubscriptionArn=sub["SubscriptionArn"]))

    snapshot.match("publish", sns.publish(
        TopicArn=arn, Message="shipped", Subject="order",
        MessageAttributes={"kind": {"DataType": "String", "StringValue": "order"}}))
    snapshot.match("envelope", json.loads(one_body(client, eventually, url)), opaque=SIGNING)


def test_raw_delivery_and_a_filter_policy(client, names, cleanup, snapshot, eventually):
    sns, sqs = client("sns"), client("sqs")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url, sub = subscribed_queue(client, names, cleanup, arn, Attributes={
        "RawMessageDelivery": "true",
        "FilterPolicy": json.dumps({"kind": ["order"]}),
    })
    snapshot.match("subscription-attributes",
                   sns.get_subscription_attributes(SubscriptionArn=sub["SubscriptionArn"]))

    kind = lambda v: {"kind": {"DataType": "String", "StringValue": v}}
    sns.publish(TopicArn=arn, Message="filtered out", MessageAttributes=kind("refund"))
    sns.publish(TopicArn=arn, Message="let through", MessageAttributes=kind("order"))

    def poll():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1, MessageAttributeNames=["All"])
        assert r.get("Messages"), "nothing delivered yet"
        return r["Messages"]

    # Raw: the body is the message, and the attributes arrive as SQS attributes.
    snapshot.match("delivered", eventually(poll), opaque=("ReceiptHandle",))


def test_refusals(client, names, cleanup, snapshot, target):
    sns = client("sns")
    absent = f"arn:aws:sns:{target.region}:{target.account}:{names('absent')}"
    snapshot.error("publish-to-no-topic", lambda: sns.publish(TopicArn=absent, Message="x"))
    snapshot.error("attributes-of-no-topic", lambda: sns.get_topic_attributes(TopicArn=absent))
    snapshot.error("bad-topic-name", lambda: sns.create_topic(Name="not a topic name!"))
    snapshot.error("not-an-arn", lambda: sns.publish(TopicArn="nonsense", Message="x"))

    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.error("empty-message", lambda: sns.publish(TopicArn=arn, Message=""))
    snapshot.error("bad-protocol", lambda: sns.subscribe(
        TopicArn=arn, Protocol="carrier-pigeon", Endpoint="x"))
    snapshot.error("bad-filter-policy", lambda: sns.subscribe(
        TopicArn=arn, Protocol="sqs",
        Endpoint=f"arn:aws:sqs:{target.region}:{target.account}:q",
        Attributes={"FilterPolicy": "{not json"}))
    snapshot.error("bad-attribute", lambda: sns.set_topic_attributes(
        TopicArn=arn, AttributeName="NoSuchAttribute", AttributeValue="x"))


def test_create_again_must_agree(client, names, cleanup, snapshot):
    """CreateTopic is idempotent for the same definition. What it does with a
    different one is recorded rather than asserted."""
    sns = client("sns")
    name = names("topic")
    arn = sns.create_topic(Name=name, Attributes={"DisplayName": "Orders"},
                           Tags=[{"Key": "env", "Value": "dev"}])["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.outcome("same", lambda: sns.create_topic(
        Name=name, Attributes={"DisplayName": "Orders"}, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.outcome("no-attributes", lambda: sns.create_topic(Name=name))
    snapshot.outcome("different-attribute", lambda: sns.create_topic(
        Name=name, Attributes={"DisplayName": "Returns"}))
    snapshot.outcome("different-tags", lambda: sns.create_topic(
        Name=name, Tags=[{"Key": "env", "Value": "prod"}]))
    snapshot.match("display-name-after", sns.get_topic_attributes(
        TopicArn=arn)["Attributes"].get("DisplayName"))


def test_publish_batch(client, names, cleanup, snapshot, eventually):
    sns, sqs = client("sns"), client("sqs")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url, _ = subscribed_queue(client, names, cleanup, arn, Attributes={"RawMessageDelivery": "true"})

    snapshot.match("publish", sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": "a", "Message": "one"}, {"Id": "b", "Message": "two"}]))
    bodies = []

    def drain():
        r = sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=10, WaitTimeSeconds=1)
        for m in r.get("Messages", []):
            bodies.append(m["Body"])
            sqs.delete_message(QueueUrl=url, ReceiptHandle=m["ReceiptHandle"])
        assert len(bodies) >= 2, f"{bodies} so far"
        return sorted(bodies)

    snapshot.match("delivered", eventually(drain))
    snapshot.error("repeated-ids", lambda: sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": "a", "Message": "one"}, {"Id": "a", "Message": "two"}]))
    snapshot.error("empty", lambda: sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[]))
    snapshot.error("eleven", lambda: sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": f"m{i}", "Message": "x"} for i in range(11)]))


def test_fifo_topic_delivers_to_a_fifo_queue(client, names, cleanup, snapshot, eventually):
    sns, sqs = client("sns"), client("sqs")
    made = snapshot.match("create", sns.create_topic(
        Name=names("topic", suffix=".fifo"),
        Attributes={"FifoTopic": "true", "ContentBasedDeduplication": "true"}))
    arn = made["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)

    url = sqs.create_queue(QueueName=names("queue", suffix=".fifo"),
                           Attributes={"FifoQueue": "true"})["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    qarn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
                       "Action": "sqs:SendMessage", "Resource": qarn,
                       "Condition": {"ArnEquals": {"aws:SourceArn": arn}}}],
    })})
    sub = sns.subscribe(TopicArn=arn, Protocol="sqs", Endpoint=qarn, ReturnSubscriptionArn=True,
                        Attributes={"RawMessageDelivery": "true"})
    cleanup(sns.unsubscribe, SubscriptionArn=sub["SubscriptionArn"])

    snapshot.match("publish", sns.publish(TopicArn=arn, Message="first", MessageGroupId="g"),
                   opaque=("SequenceNumber",))
    snapshot.error("no-group-id", lambda: sns.publish(TopicArn=arn, Message="x"))

    def poll():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1,
                                MessageSystemAttributeNames=["MessageGroupId"])
        assert r.get("Messages"), "nothing delivered yet"
        return [(m["Body"], m["Attributes"]) for m in r["Messages"]]

    snapshot.match("delivered", eventually(poll))
    snapshot.error("fifo-name-without-the-attribute", lambda: sns.create_topic(
        Name=names("plain", suffix=".fifo")))
    snapshot.error("the-attribute-without-the-fifo-name", lambda: sns.create_topic(
        Name=names("plain"), Attributes={"FifoTopic": "true"}))


def test_a_fifo_queue_on_a_standard_topic(client, names, cleanup, snapshot):
    """SNS-12 / SQS plan section 9. doze-aws accepts the subscription and then
    drops every delivery; the model says nothing of it. Whichever AWS does (SNS
    refuses a FIFO queue endpoint for a standard topic with InvalidParameter, as
    recalled) is recorded."""
    sns, sqs = client("sns"), client("sqs")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url = sqs.create_queue(QueueName=names("queue", suffix=".fifo"),
                           Attributes={"FifoQueue": "true", "ContentBasedDeduplication": "true"})["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    qarn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sub = snapshot.outcome("subscribe", lambda: sns.subscribe(
        TopicArn=arn, Protocol="sqs", Endpoint=qarn, ReturnSubscriptionArn=True), opaque=("SubscriptionArn",))
    if isinstance(sub, dict):
        cleanup(sns.unsubscribe, SubscriptionArn=sub["SubscriptionArn"])


# ---------------------------------------------------------------------------------------------------------------------
# Written ahead of the recording (A8, 2026-11-12): UNVERIFIED until recorded. Each holds what a developer's stack depends on.
# ---------------------------------------------------------------------------------------------------------------------


def policy_of(sns, arn):
    return json.loads(sns.get_topic_attributes(TopicArn=arn)["Attributes"]["Policy"])


def test_topic_permissions_attributes_and_tags(client, names, cleanup, snapshot, target, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"), Tags=[
        {"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a"}])["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.match("tags-at-creation", sns.list_tags_for_resource(ResourceArn=arn), unordered=("Tags",))
    snapshot.match("untag", sns.untag_resource(ResourceArn=arn, TagKeys=["env", "no-such-key"]))
    snapshot.match("retag", sns.tag_resource(ResourceArn=arn, Tags=[{"Key": "team", "Value": "b"}]))
    snapshot.match("tags-after", sns.list_tags_for_resource(ResourceArn=arn))
    snapshot.outcome("reserved-tag", lambda: sns.tag_resource(
        ResourceArn=arn, Tags=[{"Key": "aws:reserved", "Value": "x"}]))

    snapshot.match("add-permission", sns.add_permission(
        TopicArn=arn, Label="friends", AWSAccountId=[target.account], ActionName=["Publish", "Subscribe"]))
    snapshot.match("policy-after-add", policy_of(sns, arn))
    snapshot.outcome("add-permission-again", lambda: sns.add_permission(
        TopicArn=arn, Label="friends", AWSAccountId=[target.account], ActionName=["Publish"]))
    snapshot.outcome("add-permission-bad-action", lambda: sns.add_permission(
        TopicArn=arn, Label="other", AWSAccountId=[target.account], ActionName=["Fly"]))
    snapshot.match("remove-permission", sns.remove_permission(TopicArn=arn, Label="friends"))
    snapshot.outcome("remove-permission-again", lambda: sns.remove_permission(TopicArn=arn, Label="friends"))
    snapshot.match("policy-after-remove", policy_of(sns, arn))

    snapshot.match("set-size", sns.set_topic_attributes(
        TopicArn=arn, AttributeName="MaximumMessageSize", AttributeValue="2048"))
    snapshot.outcome("set-size-too-small", lambda: sns.set_topic_attributes(
        TopicArn=arn, AttributeName="MaximumMessageSize", AttributeValue="100"))
    snapshot.outcome("set-content-based-on-a-standard-topic", lambda: sns.set_topic_attributes(
        TopicArn=arn, AttributeName="ContentBasedDeduplication", AttributeValue="true"))
    snapshot.match("set-delivery-policy", sns.set_topic_attributes(
        TopicArn=arn, AttributeName="DeliveryPolicy",
        AttributeValue=json.dumps({"http": {"defaultHealthyRetryPolicy": {"numRetries": 5}}})))
    attrs = sns.get_topic_attributes(TopicArn=arn)["Attributes"]
    for doc in ("Policy", "EffectiveDeliveryPolicy", "DeliveryPolicy"):
        if doc in attrs:
            attrs[doc] = json.loads(attrs[doc])
    snapshot.match("attributes-after", {"Attributes": attrs})

    def listed():
        every = [t["TopicArn"] for page in sns.get_paginator("list_topics").paginate() for t in page["Topics"]]
        assert arn in every, "not listed yet"
        return [t for t in every if t == arn]

    snapshot.match("listed", eventually(listed))


def test_confirmation_is_awaited_by_https_and_not_by_a_queue(client, names, cleanup, snapshot):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.match("https", sns.subscribe(TopicArn=arn, Protocol="https", Endpoint="https://example.com/dzc"))
    pending = snapshot.match("https-with-arn", sns.subscribe(
        TopicArn=arn, Protocol="https", Endpoint="https://example.com/dzc-two", ReturnSubscriptionArn=True))
    snapshot.match("pending-attributes", sns.get_subscription_attributes(SubscriptionArn=pending["SubscriptionArn"]))
    snapshot.match("listed", sns.list_subscriptions_by_topic(TopicArn=arn), unordered=("Subscriptions",))
    snapshot.match("counts", {k: v for k, v in sns.get_topic_attributes(TopicArn=arn)["Attributes"].items()
                              if k.startswith("Subscriptions")})
    snapshot.error("wrong-token", lambda: sns.confirm_subscription(TopicArn=arn, Token="0" * 64))
    snapshot.match("unsubscribe-pending", sns.unsubscribe(SubscriptionArn=pending["SubscriptionArn"]))
    snapshot.outcome("unsubscribe-again", lambda: sns.unsubscribe(SubscriptionArn=pending["SubscriptionArn"]))


def test_subscribe_twice_and_set_subscription_attributes(client, names, cleanup, snapshot):
    sns, sqs = client("sns"), client("sqs")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url = sqs.create_queue(QueueName=names("queue"))["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    qarn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    first = sns.subscribe(TopicArn=arn, Protocol="sqs", Endpoint=qarn)["SubscriptionArn"]
    cleanup(sns.unsubscribe, SubscriptionArn=first)
    again = snapshot.match("again", sns.subscribe(TopicArn=arn, Protocol="sqs", Endpoint=qarn))
    assert again["SubscriptionArn"] == first, "the same endpoint is the same subscription"
    snapshot.outcome("again-with-attributes", lambda: sns.subscribe(
        TopicArn=arn, Protocol="sqs", Endpoint=qarn, Attributes={"RawMessageDelivery": "true"}))

    def set_attr(name, value):
        return sns.set_subscription_attributes(SubscriptionArn=first, AttributeName=name, AttributeValue=value)

    snapshot.match("set-filter", set_attr("FilterPolicy", json.dumps({"kind": ["order"], "n": [{"numeric": [">", 1]}]})))
    snapshot.match("after-filter", sns.get_subscription_attributes(SubscriptionArn=first))
    snapshot.outcome("body-scope-over-a-flat-policy", lambda: set_attr("FilterPolicyScope", "MessageBody"))
    snapshot.outcome("unknown-operator", lambda: set_attr("FilterPolicy", json.dumps({"kind": [{"nope": 1}]})))
    snapshot.outcome("exists-not-a-bool", lambda: set_attr("FilterPolicy", json.dumps({"kind": [{"exists": "yes"}]})))
    snapshot.outcome("not-an-object", lambda: set_attr("FilterPolicy", "[1]"))
    snapshot.outcome("too-complex", lambda: set_attr("FilterPolicy", json.dumps(
        {k: list(range(10)) for k in ("a", "b", "c")})))
    snapshot.outcome("bad-scope", lambda: set_attr("FilterPolicyScope", "Everything"))
    snapshot.outcome("bad-raw", lambda: set_attr("RawMessageDelivery", "sometimes"))
    snapshot.outcome("unknown", lambda: set_attr("NoSuchAttribute", "x"))
    snapshot.match("clear-filter", set_attr("FilterPolicy", ""))
    snapshot.match("after-clear", sns.get_subscription_attributes(SubscriptionArn=first))


def delivered_through(client, names, cleanup, eventually, topic_arn, send, sentinel, **subscribe):
    """Subscribes a raw queue, publishes each of `send` and then the `sentinel` (which passes), and returns the bodies that
    arrived once the sentinel had, sorted. A message a filter drops is known by its absence, so the wait is for the sentinel and a
    pause after it."""
    import time
    sqs, sns = client("sqs"), client("sns")
    url, _ = subscribed_queue(client, names, cleanup, topic_arn, Attributes={"RawMessageDelivery": "true", **subscribe})
    for kwargs in send + [sentinel]:
        sns.publish(TopicArn=topic_arn, **kwargs)
    got = []

    def poll():
        for m in sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=10, WaitTimeSeconds=1).get("Messages", []):
            got.append(m["Body"])
            sqs.delete_message(QueueUrl=url, ReceiptHandle=m["ReceiptHandle"])
        assert sentinel["Message"] in got, f"{got} so far"

    eventually(poll)
    time.sleep(2)
    poll()
    return sorted(got)


def attr(**values):
    return {k: {"DataType": "Number" if isinstance(v, (int, float)) else "String", "StringValue": str(v)}
            for k, v in values.items()}


def test_filter_policy_on_attributes(client, names, cleanup, snapshot, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    policy = {"kind": [{"prefix": "ord"}], "n": [{"numeric": [">", 1, "<=", 5]}], "tier": [{"anything-but": ["free"]}]}
    send = [
        {"Message": "passes", "MessageAttributes": attr(kind="order", n=3, tier="pro")},
        {"Message": "n too big", "MessageAttributes": attr(kind="order", n=7, tier="pro")},
        {"Message": "wrong kind", "MessageAttributes": attr(kind="refund", n=3, tier="pro")},
        {"Message": "free tier", "MessageAttributes": attr(kind="order", n=3, tier="free")},
        {"Message": "no tier", "MessageAttributes": attr(kind="order", n=3)},
    ]
    sentinel = {"Message": "sentinel", "MessageAttributes": attr(kind="order", n=2, tier="pro")}
    snapshot.match("delivered", delivered_through(
        client, names, cleanup, eventually, arn, send, sentinel, FilterPolicy=json.dumps(policy)))


def test_filter_policy_with_or_exists_and_arrays(client, names, cleanup, snapshot, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    policy = {"$or": [{"colour": ["red"]}, {"size": [{"exists": False}], "shape": [{"equals-ignore-case": "ROUND"}]}]}
    send = [
        {"Message": "red", "MessageAttributes": attr(colour="red")},
        {"Message": "round no size", "MessageAttributes": attr(shape="round")},
        {"Message": "round sized", "MessageAttributes": attr(shape="round", size="big")},
        {"Message": "blue", "MessageAttributes": attr(colour="blue")},
        {"Message": "an array", "MessageAttributes": {"colour": {"DataType": "String.Array", "StringValue": '["blue", "red"]'}}},
    ]
    sentinel = {"Message": "sentinel", "MessageAttributes": attr(colour="red")}
    snapshot.match("delivered", delivered_through(
        client, names, cleanup, eventually, arn, send, sentinel, FilterPolicy=json.dumps(policy)))


def test_filter_policy_on_the_message_body(client, names, cleanup, snapshot, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    policy = {"order": {"kind": ["rush"], "qty": [{"numeric": [">", 1]}]}, "paid": [True]}
    send = [
        {"Message": json.dumps({"order": {"kind": "rush", "qty": 3}, "paid": True})},
        {"Message": json.dumps({"order": {"kind": "rush", "qty": 1}, "paid": True})},
        {"Message": json.dumps({"order": {"kind": "slow", "qty": 3}, "paid": True})},
        {"Message": json.dumps({"order": {"kind": "rush", "qty": 3}, "paid": False})},
        {"Message": json.dumps({"order": [{"kind": "slow"}, {"kind": "rush", "qty": 9}], "paid": True})},
        {"Message": "plain text, not JSON"},
        {"Message": "attributes mean nothing here", "MessageAttributes": attr(kind="rush")},
    ]
    sentinel = {"Message": json.dumps({"order": {"kind": "rush", "qty": 2}, "paid": True, "tag": "sentinel"})}
    snapshot.match("delivered", delivered_through(
        client, names, cleanup, eventually, arn, send, sentinel,
        FilterPolicy=json.dumps(policy), FilterPolicyScope="MessageBody"))


def test_publish_checks(client, names, cleanup, snapshot):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    one = {"DataType": "String", "StringValue": "v"}
    snapshot.outcome("bad-attribute-type", lambda: sns.publish(
        TopicArn=arn, Message="x", MessageAttributes={"a": {"DataType": "Date", "StringValue": "v"}}))
    snapshot.outcome("empty-attribute-value", lambda: sns.publish(
        TopicArn=arn, Message="x", MessageAttributes={"a": {"DataType": "String", "StringValue": ""}}))
    snapshot.outcome("not-a-number", lambda: sns.publish(
        TopicArn=arn, Message="x", MessageAttributes={"a": {"DataType": "Number", "StringValue": "ten"}}))
    snapshot.outcome("eleven-attributes", lambda: sns.publish(
        TopicArn=arn, Message="x", MessageAttributes={f"a{i}": one for i in range(11)}))
    snapshot.outcome("json-without-default", lambda: sns.publish(
        TopicArn=arn, Message=json.dumps({"sqs": "x"}), MessageStructure="json"))
    snapshot.outcome("json-that-is-not-json", lambda: sns.publish(
        TopicArn=arn, Message="plain", MessageStructure="json"))
    snapshot.outcome("subject-with-a-newline", lambda: sns.publish(TopicArn=arn, Message="x", Subject="a\nb"))
    snapshot.outcome("subject-too-long", lambda: sns.publish(TopicArn=arn, Message="x", Subject="s" * 101))
    snapshot.outcome("too-long", lambda: sns.publish(TopicArn=arn, Message="x" * 262145))
    snapshot.outcome("at-the-limit", lambda: sns.publish(TopicArn=arn, Message="x" * 262144), opaque=("MessageId",))
    snapshot.outcome("no-target", lambda: sns.publish(Message="x"))
    snapshot.outcome("dedup-id-on-a-standard-topic", lambda: sns.publish(
        TopicArn=arn, Message="x", MessageDeduplicationId="d"))


def test_json_structure_picks_the_member_for_the_protocol(client, names, cleanup, snapshot, eventually):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    url, _ = subscribed_queue(client, names, cleanup, arn, Attributes={"RawMessageDelivery": "true"})
    sns.publish(TopicArn=arn, MessageStructure="json",
                Message=json.dumps({"default": "the default", "sqs": "for the queue", "email": "for email"}))
    snapshot.match("delivered", one_body(client, eventually, url))


def test_batch_reports_each_message(client, names, cleanup, snapshot):
    sns = client("sns")
    arn = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.match("mixed", sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": "ok", "Message": "fine"},
        {"Id": "bad-attribute", "Message": "x", "MessageAttributes": {"a": {"DataType": "Date", "StringValue": "v"}}},
        {"Id": "also-ok", "Message": "fine"}]), opaque=("MessageId",))
    snapshot.outcome("bad-id", lambda: sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": "has a space", "Message": "x"}]))
    snapshot.outcome("too-long", lambda: sns.publish_batch(TopicArn=arn, PublishBatchRequestEntries=[
        {"Id": "a", "Message": "x" * 150000}, {"Id": "b", "Message": "x" * 150000}]))


def test_fifo_deduplication(client, names, cleanup, snapshot, eventually):
    sns, sqs = client("sns"), client("sqs")
    arn = sns.create_topic(Name=names("topic", suffix=".fifo"), Attributes={
        "FifoTopic": "true", "ContentBasedDeduplication": "true"})["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=arn)
    snapshot.match("attributes", {k: v for k, v in sns.get_topic_attributes(TopicArn=arn)["Attributes"].items()
                                  if k in ("FifoTopic", "ContentBasedDeduplication", "FifoThroughputScope")})
    url = sqs.create_queue(QueueName=names("queue", suffix=".fifo"),
                           Attributes={"FifoQueue": "true"})["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    qarn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
                       "Action": "sqs:SendMessage", "Resource": qarn,
                       "Condition": {"ArnEquals": {"aws:SourceArn": arn}}}],
    })})
    sub = sns.subscribe(TopicArn=arn, Protocol="sqs", Endpoint=qarn, ReturnSubscriptionArn=True,
                        Attributes={"RawMessageDelivery": "true"})
    cleanup(sns.unsubscribe, SubscriptionArn=sub["SubscriptionArn"])
    first = sns.publish(TopicArn=arn, Message="same", MessageGroupId="g")
    again = sns.publish(TopicArn=arn, Message="same", MessageGroupId="g")
    snapshot.match("same-message-id-for-a-duplicate", first["MessageId"] == again["MessageId"])
    snapshot.match("same-sequence-for-a-duplicate", first["SequenceNumber"] == again["SequenceNumber"])
    snapshot.match("turn-content-based-off", sns.set_topic_attributes(
        TopicArn=arn, AttributeName="ContentBasedDeduplication", AttributeValue="false"))
    snapshot.error("no-dedup-id", lambda: sns.publish(TopicArn=arn, Message="x", MessageGroupId="g"))
    sns.publish(TopicArn=arn, Message="other", MessageGroupId="g", MessageDeduplicationId="d1")

    got = []

    def poll():
        for m in sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=10, WaitTimeSeconds=1).get("Messages", []):
            got.append(m["Body"])
            sqs.delete_message(QueueUrl=url, ReceiptHandle=m["ReceiptHandle"])
        assert "other" in got and "same" in got, f"{got} so far"

    eventually(poll)
    snapshot.match("delivered", sorted(got))
