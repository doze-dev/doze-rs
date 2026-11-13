"""EventBridge through boto3, including a delivery into SQS."""

import json


def bus(events, names, cleanup, label="bus"):
    name = names(label)
    made = events.create_event_bus(Name=name)
    cleanup(events.delete_event_bus, Name=name)
    return name, made


def target_queue(client, names, cleanup, rule_arn):
    """A queue the rule is allowed to deliver to."""
    sqs = client("sqs")
    url = sqs.create_queue(QueueName=names("queue"))["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow", "Principal": {"Service": "events.amazonaws.com"},
            "Action": "sqs:SendMessage", "Resource": arn,
            "Condition": {"ArnEquals": {"aws:SourceArn": rule_arn}},
        }],
    })})
    return url, arn


def test_bus_and_rule_lifecycle(client, names, cleanup, snapshot):
    events = client("events")
    name, made = bus(events, names, cleanup)
    snapshot.match("create-bus", made)
    snapshot.match("describe-bus", events.describe_event_bus(Name=name))

    rule = names("rule")
    pattern = json.dumps({"source": ["shop.orders"], "detail-type": ["placed"]})
    snapshot.match("put-rule", events.put_rule(
        Name=rule, EventBusName=name, EventPattern=pattern, Description="orders"))
    cleanup(events.delete_rule, Name=rule, EventBusName=name)
    snapshot.match("describe-rule", events.describe_rule(Name=rule, EventBusName=name))
    snapshot.match("list-rules", events.list_rules(EventBusName=name))

    snapshot.match("disable", events.disable_rule(Name=rule, EventBusName=name))
    snapshot.match("disabled", events.describe_rule(Name=rule, EventBusName=name))
    snapshot.match("enable", events.enable_rule(Name=rule, EventBusName=name))

    rule_arn = events.describe_rule(Name=rule, EventBusName=name)["Arn"]
    snapshot.match("tag-rule", events.tag_resource(
        ResourceARN=rule_arn, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("rule-tags", events.list_tags_for_resource(ResourceARN=rule_arn))

    snapshot.match("delete-rule", events.delete_rule(Name=rule, EventBusName=name))
    snapshot.match("delete-bus", events.delete_event_bus(Name=name))
    snapshot.outcome("delete-bus-again", lambda: events.delete_event_bus(Name=name))


def test_a_bus_can_be_tagged(client, names, cleanup, snapshot):
    events = client("events")
    _, made = bus(events, names, cleanup)
    arn = made["EventBusArn"]
    snapshot.match("tag", events.tag_resource(ResourceARN=arn, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("tags", events.list_tags_for_resource(ResourceARN=arn))
    snapshot.match("untag", events.untag_resource(ResourceARN=arn, TagKeys=["env"]))
    snapshot.match("no-tags", events.list_tags_for_resource(ResourceARN=arn))


def test_a_matching_event_reaches_the_queue(client, names, cleanup, snapshot, eventually):
    events, sqs = client("events"), client("sqs")
    name, _ = bus(events, names, cleanup)
    rule = names("rule")
    rule_arn = events.put_rule(
        Name=rule, EventBusName=name,
        EventPattern=json.dumps({"source": ["shop.orders"], "detail": {"total": [{"numeric": [">", 100]}]}}),
    )["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=name)
    url, arn = target_queue(client, names, cleanup, rule_arn)

    snapshot.match("put-targets", events.put_targets(
        Rule=rule, EventBusName=name, Targets=[{"Id": "queue", "Arn": arn}]))
    cleanup(events.remove_targets, Rule=rule, EventBusName=name, Ids=["queue"])
    snapshot.match("list-targets", events.list_targets_by_rule(Rule=rule, EventBusName=name))

    def entry(total):
        return {"EventBusName": name, "Source": "shop.orders", "DetailType": "placed",
                "Detail": json.dumps({"total": total}), "Resources": ["arn:aws:x:::order/1"]}

    snapshot.match("put-events", events.put_events(Entries=[entry(50), entry(250)]),
                   opaque=("EventId",))

    def poll():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1)
        assert r.get("Messages"), "nothing delivered yet"
        return json.loads(r["Messages"][0]["Body"])

    # Only the second entry matches the numeric filter.
    snapshot.match("delivered", eventually(poll, timeout=60), opaque=("time",))

    snapshot.match("remove-targets", events.remove_targets(
        Rule=rule, EventBusName=name, Ids=["queue", "absent"]))


def test_a_rule_with_targets_cannot_be_deleted(client, names, cleanup, snapshot, target):
    events = client("events")
    name, _ = bus(events, names, cleanup)
    rule = names("rule")
    events.put_rule(Name=rule, EventBusName=name, EventPattern=json.dumps({"source": ["x"]}))
    cleanup(events.delete_rule, Name=rule, EventBusName=name)
    events.put_targets(Rule=rule, EventBusName=name, Targets=[
        # A queue in another account needs a RoleArn on real AWS, so the target is in the run's own account.
        {"Id": "t", "Arn": f"arn:aws:sqs:us-east-1:{target.account}:anything"}])
    cleanup(events.remove_targets, Rule=rule, EventBusName=name, Ids=["t"])
    snapshot.error("delete-rule-with-targets", lambda: events.delete_rule(
        Name=rule, EventBusName=name))
    snapshot.outcome("still-there", lambda: events.describe_rule(Name=rule, EventBusName=name))


def test_input_transformer(client, names, cleanup, snapshot, eventually):
    events, sqs = client("events"), client("sqs")
    name, _ = bus(events, names, cleanup)
    rule = names("rule")
    rule_arn = events.put_rule(Name=rule, EventBusName=name,
                               EventPattern=json.dumps({"source": ["shop.orders"]}))["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=name)
    url, arn = target_queue(client, names, cleanup, rule_arn)
    events.put_targets(Rule=rule, EventBusName=name, Targets=[{
        "Id": "queue", "Arn": arn,
        "InputTransformer": {
            "InputPathsMap": {"who": "$.detail.customer", "kind": "$.detail-type"},
            "InputTemplate": '{"message": "<who> did <kind>"}',
        },
    }])
    cleanup(events.remove_targets, Rule=rule, EventBusName=name, Ids=["queue"])
    events.put_events(Entries=[{"EventBusName": name, "Source": "shop.orders",
                                "DetailType": "placed", "Detail": json.dumps({"customer": "ada"})}])

    def poll():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1)
        assert r.get("Messages"), "nothing delivered yet"
        return r["Messages"][0]["Body"]

    snapshot.match("transformed", eventually(poll, timeout=60))


def test_pattern_matching(client, snapshot):
    """TestEventPattern is the pattern language with nothing else attached —
    no bus, no rule, no delivery — so every case is one comparison."""
    events = client("events")
    event = {
        "id": "1", "account": "123456789012", "source": "shop.orders",
        "time": "2026-01-01T00:00:00Z", "region": "us-east-1", "resources": [],
        "detail-type": "placed",
        "detail": {"total": 250, "customer": {"tier": "gold", "name": "Ada"},
                   "items": ["book", "pen"], "note": None, "paid": True},
    }
    patterns = {
        "exact": {"source": ["shop.orders"]},
        "miss": {"source": ["shop.returns"]},
        "prefix": {"source": [{"prefix": "shop."}]},
        "suffix": {"source": [{"suffix": ".orders"}]},
        "anything-but": {"detail": {"customer": {"tier": [{"anything-but": ["bronze"]}]}}},
        "anything-but-miss": {"detail": {"customer": {"tier": [{"anything-but": ["gold"]}]}}},
        "numeric-range": {"detail": {"total": [{"numeric": [">=", 100, "<", 300]}]}},
        "numeric-miss": {"detail": {"total": [{"numeric": ["<", 100]}]}},
        "exists": {"detail": {"customer": {"name": [{"exists": True}]}}},
        "not-exists": {"detail": {"coupon": [{"exists": False}]}},
        "array-member": {"detail": {"items": ["pen"]}},
        "null": {"detail": {"note": [None]}},
        "boolean": {"detail": {"paid": [True]}},
        "equals-ignore-case": {"detail": {"customer": {"name": [{"equals-ignore-case": "ADA"}]}}},
        "wildcard": {"source": [{"wildcard": "shop.*"}]},
        "or": {"$or": [{"source": ["nope"]}, {"detail-type": ["placed"]}]},
        "two-fields-and": {"source": ["shop.orders"], "detail-type": ["refunded"]},
    }
    for label, pattern in patterns.items():
        snapshot.match(label, events.test_event_pattern(
            Event=json.dumps(event), EventPattern=json.dumps(pattern)))


# The scenarios from here on were written after the recording of 2026-11-09 and have none: they run against doze-aws as UNVERIFIED
# until they are recorded (CONFORMANCE_TARGET=aws CONFORMANCE_RECORD=1; read each diff, where AWS contradicts the build, AWS wins).


def test_schedule_expressions(client, names, cleanup, snapshot):
    events = client("events")
    rule = names("rule")
    for label, expression in [
        ("rate-minutes", "rate(5 minutes)"), ("rate-minute", "rate(1 minute)"), ("rate-hours", "rate(2 hours)"),
        ("rate-day", "rate(1 day)"), ("cron", "cron(0 12 * * ? *)"), ("cron-steps", "cron(0/15 8-17 ? * MON-FRI *)"),
        ("cron-last", "cron(0 0 L * ? *)"), ("cron-nth", "cron(0 18 ? * 6#3 *)"),
    ]:
        snapshot.match(label, events.put_rule(Name=rule, ScheduleExpression=expression, State="DISABLED"))
        cleanup(events.delete_rule, Name=rule)
    snapshot.match("described", events.describe_rule(Name=rule))
    for label, expression in [
        ("no-unit", "rate(5)"), ("zero", "rate(0 minutes)"), ("weeks", "rate(1 week)"), ("both-days", "cron(0 12 * * * *)"),
        ("neither-day", "cron(0 12 ? * ? *)"), ("five-fields", "cron(0 12 * * ?)"), ("minute-out-of-range", "cron(60 12 * * ? *)"),
        ("not-an-expression", "every day"),
    ]:
        snapshot.error(label, lambda e=expression: events.put_rule(Name=rule, ScheduleExpression=e))


def test_target_parameters_come_back_as_sent(client, names, cleanup, snapshot):
    events = client("events")
    rule = names("rule")
    events.put_rule(Name=rule, EventPattern=json.dumps({"source": ["x"]}), State="DISABLED")
    cleanup(events.delete_rule, Name=rule)
    account = client("sts").get_caller_identity()["Account"]
    queue = f"arn:aws:sqs:us-east-1:{account}:{names('q')}"
    targets = [
        {"Id": "constant", "Arn": queue, "Input": '{"fixed": true}'},
        {"Id": "path", "Arn": queue, "InputPath": "$.detail"},
        {"Id": "fifo", "Arn": queue + ".fifo", "SqsParameters": {"MessageGroupId": "group"}},
        {"Id": "retry", "Arn": queue, "RetryPolicy": {"MaximumRetryAttempts": 3, "MaximumEventAgeInSeconds": 120},
         "DeadLetterConfig": {"Arn": queue + "-dlq"}},
    ]
    snapshot.match("put", events.put_targets(Rule=rule, Targets=targets))
    cleanup(events.remove_targets, Rule=rule, Ids=[t["Id"] for t in targets])
    listed = events.list_targets_by_rule(Rule=rule)["Targets"]
    snapshot.match("listed", {"Targets": sorted(listed, key=lambda t: t["Id"])})
    snapshot.match("by-target", events.list_rule_names_by_target(TargetArn=queue))
    snapshot.outcome("input-and-path", lambda: events.put_targets(Rule=rule, Targets=[
        {"Id": "both", "Arn": queue, "Input": "{}", "InputPath": "$.detail"}]))


def test_a_bus_policy(client, names, cleanup, snapshot):
    events = client("events")
    name, made = bus(events, names, cleanup)
    account = client("sts").get_caller_identity()["Account"]
    snapshot.match("put-permission", events.put_permission(
        EventBusName=name, StatementId="peer", Action="events:PutEvents", Principal="444455556666"))
    policy = json.loads(events.describe_event_bus(Name=name)["Policy"])
    snapshot.match("policy", {"Statement": policy["Statement"], "Version": policy["Version"]})
    snapshot.error("statement-again", lambda: events.put_permission(
        EventBusName=name, StatementId="peer", Action="events:PutEvents", Principal="444455556666"))
    snapshot.match("remove-permission", events.remove_permission(EventBusName=name, StatementId="peer"))
    snapshot.error("remove-again", lambda: events.remove_permission(EventBusName=name, StatementId="peer"))
    assert account


def test_input_settings_reach_the_queue(client, names, cleanup, snapshot, eventually):
    events, sqs = client("events"), client("sqs")
    name, _ = bus(events, names, cleanup)
    rule = names("rule")
    rule_arn = events.put_rule(Name=rule, EventBusName=name, EventPattern=json.dumps({"source": ["shop.orders"]}))["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=name)
    url, arn = target_queue(client, names, cleanup, rule_arn)
    cleanup(events.remove_targets, Rule=rule, EventBusName=name, Ids=["queue"])
    entry = {"EventBusName": name, "Source": "shop.orders", "DetailType": "placed",
             "Detail": json.dumps({"customer": "ada", "total": 250, "items": ["book", "pen"]})}

    def delivered():
        r = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1)
        assert r.get("Messages"), "nothing delivered yet"
        sqs.delete_message(QueueUrl=url, ReceiptHandle=r["Messages"][0]["ReceiptHandle"])
        return r["Messages"][0]["Body"]

    for label, settings in [
        ("constant", {"Input": '{"fixed":  true}'}),
        ("path", {"InputPath": "$.detail.items"}),
        ("path-scalar", {"InputPath": "$.detail.customer"}),
        ("transformer-json", {"InputTransformer": {
            "InputPathsMap": {"who": "$.detail.customer", "n": "$.detail.total", "all": "$.detail.items"},
            "InputTemplate": '{"who": "<who>", "n": <n>, "items": <all>, "rule": "<aws.events.rule-name>"}'}}),
        ("transformer-text", {"InputTransformer": {
            "InputPathsMap": {"who": "$.detail.customer"}, "InputTemplate": "hello <who>"}}),
    ]:
        events.put_targets(Rule=rule, EventBusName=name, Targets=[{"Id": "queue", "Arn": arn, **settings}])
        events.put_events(Entries=[entry])
        body = eventually(delivered, timeout=60)
        snapshot.match(label, {"Body": body.replace(rule, "<rule>")})


def test_refusals(client, names, cleanup, snapshot):
    events = client("events")
    name, _ = bus(events, names, cleanup)
    rule = names("rule")
    snapshot.error("create-bus-twice", lambda: events.create_event_bus(Name=name))
    snapshot.error("describe-absent-bus", lambda: events.describe_event_bus(Name=names("absent")))
    snapshot.error("describe-absent-rule", lambda: events.describe_rule(
        Name=rule, EventBusName=name))
    snapshot.error("rule-on-absent-bus", lambda: events.put_rule(
        Name=rule, EventBusName=names("absent"), EventPattern='{"source":["x"]}'))
    snapshot.error("pattern-not-json", lambda: events.put_rule(
        Name=rule, EventBusName=name, EventPattern="{not json"))
    snapshot.error("pattern-value-not-in-array", lambda: events.put_rule(
        Name=rule, EventBusName=name, EventPattern='{"source": "x"}'))
    snapshot.error("neither-pattern-nor-schedule", lambda: events.put_rule(
        Name=rule, EventBusName=name))
    snapshot.error("schedule-on-a-custom-bus", lambda: events.put_rule(
        Name=rule, EventBusName=name, ScheduleExpression="rate(5 minutes)"))
    # These two would land on the default bus if they were ever accepted.
    cleanup(events.delete_rule, Name=rule)
    snapshot.error("plural-unit-for-one", lambda: events.put_rule(
        Name=rule, ScheduleExpression="rate(1 minutes)"))
    snapshot.error("singular-unit-for-many", lambda: events.put_rule(
        Name=rule, ScheduleExpression="rate(5 minute)"))
    snapshot.error("targets-on-absent-rule", lambda: events.put_targets(
        Rule=names("absent-rule"), EventBusName=name,
        Targets=[{"Id": "t", "Arn": "arn:aws:sqs:us-east-1:000000000000:q"}]))
    snapshot.error("delete-default-bus", lambda: events.delete_event_bus(Name="default"))
    # An entry that cannot be accepted fails on its own; the call succeeds.
    snapshot.match("entry-without-source", events.put_events(Entries=[
        {"EventBusName": name, "DetailType": "x", "Detail": "{}"}]))
    snapshot.match("entry-with-bad-detail", events.put_events(Entries=[
        {"EventBusName": name, "Source": "s", "DetailType": "x", "Detail": "not json"}]))
