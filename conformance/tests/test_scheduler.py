"""EventBridge Scheduler through boto3.

The labelled answers are UNVERIFIED until recorded against AWS (CONFORMANCE_TARGET=aws CONFORMANCE_RECORD=1): schedules and groups,
their defaults, listing and paging, tags, and the refusals. The last scenario fires a one-time schedule into SQS through doze's local
hook (POST /_doze/scheduler/<region>/<group>/<name>/fire) and runs against doze-aws only: on AWS it would wait for the minute.
"""

import json
import time
import urllib.request

import pytest

# Clocks: when the schedule or group was made or changed.
MINTED = ("CreationDate", "LastModificationDate")


def at_in(seconds):
    return "at(" + time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(time.time() + seconds)) + ")"


def queue(client, names, cleanup, label="queue", **attrs):
    sqs = client("sqs")
    url = sqs.create_queue(QueueName=names(label), Attributes=attrs)["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    return url, arn


def role(service_role, *actions):
    return service_role("scheduler", inline={
        "Version": "2012-10-17",
        "Statement": [{"Effect": "Allow", "Action": list(actions), "Resource": "*"}],
    })


def test_schedule_lifecycle(client, names, cleanup, snapshot, service_role):
    scheduler = client("scheduler")
    _, arn = queue(client, names, cleanup)
    target = {"Arn": arn, "RoleArn": role(service_role, "sqs:SendMessage"), "Input": "hello"}
    name = names("schedule")
    made = scheduler.create_schedule(
        Name=name, ScheduleExpression="cron(0 9 ? * MON-FRI *)", ScheduleExpressionTimezone="Europe/Paris",
        FlexibleTimeWindow={"Mode": "FLEXIBLE", "MaximumWindowInMinutes": 15}, Target=target,
        Description="weekday mornings")
    cleanup(scheduler.delete_schedule, Name=name)
    snapshot.match("create", made)
    snapshot.match("get", scheduler.get_schedule(Name=name), opaque=MINTED)
    snapshot.match("update", scheduler.update_schedule(
        Name=name, ScheduleExpression="rate(1 hour)", FlexibleTimeWindow={"Mode": "OFF"}, Target=target,
        State="DISABLED"))
    snapshot.match("get-after-update", scheduler.get_schedule(Name=name), opaque=MINTED)
    listed = scheduler.list_schedules(NamePrefix=name)
    snapshot.match("list", listed["Schedules"], opaque=MINTED)
    snapshot.match("list-enabled", scheduler.list_schedules(NamePrefix=name, State="ENABLED")["Schedules"])
    snapshot.match("delete", scheduler.delete_schedule(Name=name))
    snapshot.error("get-deleted", lambda: scheduler.get_schedule(Name=name))
    snapshot.error("delete-deleted", lambda: scheduler.delete_schedule(Name=name))


def test_groups_and_tags(client, names, cleanup, snapshot, service_role):
    scheduler = client("scheduler")
    group = names("group")
    made = scheduler.create_schedule_group(Name=group, Tags=[{"Key": "team", "Value": "a"}])
    cleanup(scheduler.delete_schedule_group, Name=group)
    snapshot.match("create-group", made)
    snapshot.error("create-group-again", lambda: scheduler.create_schedule_group(Name=group))
    snapshot.match("get-group", scheduler.get_schedule_group(Name=group), opaque=MINTED)
    snapshot.match("get-default", scheduler.get_schedule_group(Name="default"), opaque=MINTED)
    arn = made["ScheduleGroupArn"]
    snapshot.match("tag", scheduler.tag_resource(ResourceArn=arn, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("untag", scheduler.untag_resource(ResourceArn=arn, TagKeys=["team"]))
    snapshot.match("tags", scheduler.list_tags_for_resource(ResourceArn=arn))
    _, qarn = queue(client, names, cleanup)
    target = {"Arn": qarn, "RoleArn": role(service_role, "sqs:SendMessage")}
    name = names("in-group")
    scheduler.create_schedule(Name=name, GroupName=group, ScheduleExpression="rate(1 day)",
                              FlexibleTimeWindow={"Mode": "OFF"}, Target=target)
    snapshot.match("list-in-group", [s["Name"] for s in scheduler.list_schedules(GroupName=group)["Schedules"]])
    snapshot.match("delete-group", scheduler.delete_schedule_group(Name=group))
    snapshot.error("default-cannot-go", lambda: scheduler.delete_schedule_group(Name="default"))


def test_refusals(client, names, cleanup, snapshot, service_role):
    scheduler = client("scheduler")
    _, arn = queue(client, names, cleanup)
    target = {"Arn": arn, "RoleArn": role(service_role, "sqs:SendMessage")}
    off = {"Mode": "OFF"}

    def create(**kw):
        args = {"Name": names("refused"), "ScheduleExpression": "rate(1 hour)", "FlexibleTimeWindow": off,
                "Target": target, **kw}
        return lambda: scheduler.create_schedule(**args)

    snapshot.error("bad-expression", create(ScheduleExpression="rate(5 minute)"))
    snapshot.error("bad-at", create(ScheduleExpression="at(2030-02-30T00:00:00)"))
    snapshot.error("bad-timezone", create(ScheduleExpression="cron(0 9 * * ? *)", ScheduleExpressionTimezone="Mars/Olympus"))
    snapshot.error("end-in-past", create(EndDate=time.time() - 3600))
    snapshot.error("flexible-without-window", create(FlexibleTimeWindow={"Mode": "FLEXIBLE"}))
    snapshot.error("no-such-group", create(GroupName=names("missing")))
    snapshot.error("read-only-universal", create(Target={**target, "Arn": "arn:aws:scheduler:::aws-sdk:sqs:getQueueUrl"}))
    name = names("twice")
    scheduler.create_schedule(Name=name, ScheduleExpression="rate(1 hour)", FlexibleTimeWindow=off, Target=target)
    cleanup(scheduler.delete_schedule, Name=name)
    snapshot.error("create-twice", lambda: scheduler.create_schedule(
        Name=name, ScheduleExpression="rate(1 hour)", FlexibleTimeWindow=off, Target=target))


@pytest.fixture
def local_only(target):
    if target.name == "aws":
        pytest.skip("doze's fire-now hook does not exist on AWS")


def fire(target, group, name):
    url = f"{target.endpoint}/_doze/scheduler/{target.region}/{group}/{name}/fire"
    with urllib.request.urlopen(urllib.request.Request(url, method="POST", data=b"")) as r:
        return json.loads(r.read())


def test_a_one_minute_at_schedule_fires_into_sqs_through_the_hook(client, names, cleanup, service_role, target, local_only):
    scheduler, sqs = client("scheduler"), client("sqs")
    url, arn = queue(client, names, cleanup)
    name = names("once")
    made = scheduler.create_schedule(
        Name=name, ScheduleExpression=at_in(60), FlexibleTimeWindow={"Mode": "OFF"}, ActionAfterCompletion="DELETE",
        Target={"Arn": arn, "RoleArn": role(service_role, "sqs:SendMessage"),
                "Input": "from <aws.scheduler.schedule-arn> attempt <aws.scheduler.attempt-number>"})
    cleanup(scheduler.delete_schedule, Name=name)
    answer = fire(target, "default", name)
    assert answer["Outcome"] == "DELIVERED", answer
    got = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1)["Messages"]
    assert got[0]["Body"] == f"from {made['ScheduleArn']} attempt 1"
    # The fire was the one-time schedule's one fire, and it asked to be deleted after it.
    with pytest.raises(scheduler.exceptions.ResourceNotFoundException):
        scheduler.get_schedule(Name=name)


def test_a_failed_invocation_lands_in_the_dead_letter_queue(client, names, cleanup, service_role, target, local_only):
    scheduler, sqs = client("scheduler"), client("sqs")
    dlq_url, dlq_arn = queue(client, names, cleanup, "dlq")
    gone = dlq_arn.rsplit(":", 1)[0] + ":" + names("gone")
    name = names("doomed")
    made = scheduler.create_schedule(
        Name=name, ScheduleExpression="rate(1 day)", FlexibleTimeWindow={"Mode": "OFF"},
        Target={"Arn": gone, "RoleArn": role(service_role, "sqs:SendMessage"), "Input": "lost",
                "DeadLetterConfig": {"Arn": dlq_arn}, "RetryPolicy": {"MaximumRetryAttempts": 0}})
    cleanup(scheduler.delete_schedule, Name=name)
    assert fire(target, "default", name)["Outcome"] == "FAILED"
    got = sqs.receive_message(QueueUrl=dlq_url, MessageAttributeNames=["All"], WaitTimeSeconds=2)["Messages"][0]
    attrs = {k: v["StringValue"] for k, v in got["MessageAttributes"].items()}
    assert got["Body"] == "lost"
    assert attrs["SCHEDULE_ARN"] == made["ScheduleArn"] and attrs["TARGET_ARN"] == gone
    assert attrs["ERROR_CODE"] == "AWS.SimpleQueueService.NonExistentQueue"


def test_a_rate_schedule_fires_on_its_own(client, names, cleanup, service_role, eventually, target):
    """No hook: a rate without a StartDate starts invoking at once (documented), so the timer delivers it."""
    scheduler, sqs = client("scheduler"), client("sqs")
    url, arn = queue(client, names, cleanup)
    name = names("rate")
    scheduler.create_schedule(
        Name=name, ScheduleExpression="rate(1 hour)", FlexibleTimeWindow={"Mode": "OFF"},
        Target={"Arn": arn, "RoleArn": role(service_role, "sqs:SendMessage"), "Input": "tick"})
    cleanup(scheduler.delete_schedule, Name=name)

    def arrived():
        got = sqs.receive_message(QueueUrl=url, WaitTimeSeconds=1).get("Messages", [])
        assert got, "not yet"
        return got

    assert eventually(arrived, timeout=120 if target.name == "aws" else 10)[0]["Body"] == "tick"
