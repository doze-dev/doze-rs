"""EventBridge archives, replay, bus-to-bus delivery, dead-letter queues, connections and API destinations, through boto3, against
doze-aws only.

These are NOT compared with AWS and have no recording: an archive takes minutes to begin keeping events and a replay is asynchronous
there (doze-aws finishes one before it answers, SCOPE.md), a bus-to-bus target needs an IAM role, a dead letter needs a failure AWS
retries for hours, and a connection needs Secrets Manager. What these hold is that boto3 can drive the calls and the state comes back.
Against real AWS they are skipped. They count in no fidelity score.
"""

import json
import time

import pytest


@pytest.fixture
def local_only(target):
    if target.name == "aws":
        pytest.skip("asynchronous, slow or role-bound on AWS; doze-aws only")


def queue_for(client, names, cleanup, rule_arn):
    sqs = client("sqs")
    url = sqs.create_queue(QueueName=names("queue"))["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Principal": {"Service": "events.amazonaws.com"},
                       "Action": "sqs:SendMessage", "Resource": arn,
                       "Condition": {"ArnEquals": {"aws:SourceArn": rule_arn}}}],
    })})
    return url, arn


def bodies(client, url):
    got = client("sqs").receive_message(QueueUrl=url, MaxNumberOfMessages=10)
    return [json.loads(m["Body"]) for m in got.get("Messages", [])]


def test_an_archive_keeps_events_and_a_replay_puts_them_back(client, names, cleanup, local_only):
    events = client("events")
    bus = names("bus")
    bus_arn = events.create_event_bus(Name=bus)["EventBusArn"]
    cleanup(events.delete_event_bus, Name=bus)
    archive = names("archive")
    archive_arn = events.create_archive(
        ArchiveName=archive, EventSourceArn=bus_arn, EventPattern=json.dumps({"source": ["keep"]}))["ArchiveArn"]
    cleanup(events.delete_archive, ArchiveName=archive)
    start = time.time() - 60
    for source in ("keep", "drop", "keep"):
        events.put_events(Entries=[{"EventBusName": bus, "Source": source, "DetailType": "x", "Detail": "{}"}])
    assert events.describe_archive(ArchiveName=archive)["EventCount"] == 2
    rule = names("rule")
    rule_arn = events.put_rule(Name=rule, EventBusName=bus, EventPattern=json.dumps({"source": ["keep"]}))["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=bus)
    url, arn = queue_for(client, names, cleanup, rule_arn)
    events.put_targets(Rule=rule, EventBusName=bus, Targets=[{"Id": "q", "Arn": arn}])
    cleanup(events.remove_targets, Rule=rule, EventBusName=bus, Ids=["q"])
    replay = names("replay")
    events.start_replay(
        ReplayName=replay, EventSourceArn=archive_arn, EventStartTime=start, EventEndTime=time.time() + 60,
        Destination={"Arn": bus_arn})
    got = bodies(client, url)
    assert len(got) == 2 and all(e["replay-name"] == replay for e in got)
    assert events.describe_replay(ReplayName=replay)["State"] == "COMPLETED"
    assert [r["ReplayName"] for r in events.list_replays(NamePrefix=replay)["Replays"]] == [replay]


def test_an_event_bus_can_be_a_target(client, names, cleanup, local_only):
    events = client("events")
    first, second = names("first"), names("second")
    second_arn = events.create_event_bus(Name=second)["EventBusArn"]
    events.create_event_bus(Name=first)
    cleanup(events.delete_event_bus, Name=first)
    cleanup(events.delete_event_bus, Name=second)
    forward = names("forward")
    events.put_rule(Name=forward, EventBusName=first, EventPattern=json.dumps({"source": ["x"]}))
    cleanup(events.delete_rule, Name=forward, EventBusName=first)
    events.put_targets(Rule=forward, EventBusName=first, Targets=[{"Id": "bus", "Arn": second_arn}])
    cleanup(events.remove_targets, Rule=forward, EventBusName=first, Ids=["bus"])
    rule = names("rule")
    rule_arn = events.put_rule(Name=rule, EventBusName=second, EventPattern=json.dumps({"source": ["x"]}))["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=second)
    url, arn = queue_for(client, names, cleanup, rule_arn)
    events.put_targets(Rule=rule, EventBusName=second, Targets=[{"Id": "q", "Arn": arn}])
    cleanup(events.remove_targets, Rule=rule, EventBusName=second, Ids=["q"])
    events.put_events(Entries=[{"EventBusName": first, "Source": "x", "DetailType": "x", "Detail": '{"n": 1}'}])
    assert [e["detail"] for e in bodies(client, url)] == [{"n": 1}]


def test_what_cannot_be_delivered_lands_in_the_dead_letter_queue(client, names, cleanup, local_only):
    events, sqs = client("events"), client("sqs")
    bus = names("bus")
    events.create_event_bus(Name=bus)
    cleanup(events.delete_event_bus, Name=bus)
    rule = names("rule")
    rule_arn = events.put_rule(Name=rule, EventBusName=bus, EventPattern=json.dumps({"source": ["x"]}))["RuleArn"]
    cleanup(events.delete_rule, Name=rule, EventBusName=bus)
    dlq_url, dlq_arn = queue_for(client, names, cleanup, rule_arn)
    gone = dlq_arn.rsplit(":", 1)[0] + ":" + names("gone")
    events.put_targets(Rule=rule, EventBusName=bus, Targets=[{"Id": "t", "Arn": gone, "DeadLetterConfig": {"Arn": dlq_arn}}])
    cleanup(events.remove_targets, Rule=rule, EventBusName=bus, Ids=["t"])
    events.put_events(Entries=[{"EventBusName": bus, "Source": "x", "DetailType": "x", "Detail": "{}"}])
    got = sqs.receive_message(QueueUrl=dlq_url, MessageAttributeNames=["All"])["Messages"][0]
    assert json.loads(got["Body"])["source"] == "x"
    attrs = {k: v["StringValue"] for k, v in got["MessageAttributes"].items()}
    assert attrs["RULE_ARN"] == rule_arn and attrs["TARGET_ARN"] == gone
    assert attrs["ERROR_CODE"] and attrs["ERROR_MESSAGE"] and attrs["RETRY_ATTEMPTS"] == "0"


def test_a_connection_keeps_its_secrets_and_a_destination_follows_it(client, names, cleanup, local_only):
    events = client("events")
    conn = names("conn")
    made = events.create_connection(Name=conn, AuthorizationType="BASIC", AuthParameters={
        "BasicAuthParameters": {"Username": "ada", "Password": "hunter2"},
        "InvocationHttpParameters": {"HeaderParameters": [{"Key": "X-Token", "Value": "hidden", "IsValueSecret": True}]}})
    cleanup(events.delete_connection, Name=conn)
    described = events.describe_connection(Name=conn)
    assert described["ConnectionState"] == "AUTHORIZED"
    shown = json.dumps(described["AuthParameters"])
    assert "ada" in shown and "hunter2" not in shown and "hidden" not in shown
    dest = names("dest")
    events.create_api_destination(
        Name=dest, ConnectionArn=made["ConnectionArn"], InvocationEndpoint="http://localhost:9/hook", HttpMethod="POST")
    cleanup(events.delete_api_destination, Name=dest)
    assert events.describe_api_destination(Name=dest)["ApiDestinationState"] == "ACTIVE"
    events.deauthorize_connection(Name=conn)
    assert events.describe_connection(Name=conn)["ConnectionState"] == "DEAUTHORIZED"
    assert events.describe_api_destination(Name=dest)["ApiDestinationState"] == "INACTIVE"
