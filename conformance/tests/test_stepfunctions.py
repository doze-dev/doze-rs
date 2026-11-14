"""Step Functions through boto3.

Most of this is the States Language itself: a definition and an input go in,
an output comes out, and nothing but the interpreter is involved. That makes
it the cleanest comparison in the suite — no clocks, no ids, no delivery.
"""

import json

# Tokens and clocks. The ARNs are made of names this run chose, and are compared.
MINTED = ("startDate", "stopDate", "creationDate", "updateDate", "timestamp", "redriveDate")


def machine(sfn, names, cleanup, role, definition, label="machine", **kw):
    made = sfn.create_state_machine(
        name=names(label), definition=json.dumps(definition), roleArn=role, **kw)
    cleanup(sfn.delete_state_machine, stateMachineArn=made["stateMachineArn"])
    return made["stateMachineArn"], made


def run(sfn, names, eventually, arn, payload, label="run"):
    """Start an execution and wait for it to stop."""
    started = sfn.start_execution(stateMachineArn=arn, name=names(label), input=json.dumps(payload))

    def finished():
        d = sfn.describe_execution(executionArn=started["executionArn"])
        assert d["status"] != "RUNNING", "still running"
        return d

    d = eventually(finished, timeout=120)
    for doc in ("input", "output"):
        if doc in d:
            d[doc] = json.loads(d[doc])
    return started, d


def test_machine_lifecycle_and_history(client, names, cleanup, snapshot, service_role, eventually):
    sfn = client("stepfunctions")
    definition = {
        "Comment": "conformance", "StartAt": "Greet",
        "States": {"Greet": {"Type": "Pass", "Parameters": {"hello.$": "$.who"}, "End": True}},
    }
    arn, made = machine(sfn, names, cleanup, service_role("states"), definition)
    snapshot.match("create", made, opaque=MINTED)
    described = sfn.describe_state_machine(stateMachineArn=arn)
    described["definition"] = json.loads(described["definition"])
    snapshot.match("describe", described, opaque=MINTED, drop=("revisionId",))

    started, done = run(sfn, names, eventually, arn, {"who": "world"})
    snapshot.match("start", started, opaque=MINTED)
    snapshot.match("done", done, opaque=MINTED, drop=("redriveCount", "redriveStatus", "redriveStatusReason"))
    history = sfn.get_execution_history(executionArn=started["executionArn"])
    snapshot.match("history", [e["type"] for e in history["events"]])
    snapshot.match("history-links", [(e["id"], e["previousEventId"]) for e in history["events"]])
    snapshot.match("list", [e["name"] for e in sfn.list_executions(stateMachineArn=arn)["executions"]])

    snapshot.match("update", sfn.update_state_machine(
        stateMachineArn=arn, definition=json.dumps({**definition, "Comment": "changed"})),
        opaque=MINTED, drop=("revisionId",))
    snapshot.match("delete", sfn.delete_state_machine(stateMachineArn=arn))


def test_the_states_language(client, names, cleanup, snapshot, service_role, eventually):
    """One machine per feature, each a pure function of its input."""
    sfn = client("stepfunctions")
    role = service_role("states")
    machines = {
        "paths": ({
            "StartAt": "Shape", "States": {"Shape": {
                "Type": "Pass", "InputPath": "$.order",
                "Parameters": {"id.$": "$.id", "first.$": "$.items[0]", "count.$": "States.ArrayLength($.items)"},
                "ResultPath": "$.summary", "OutputPath": "$.summary", "End": True}},
        }, {"order": {"id": 7, "items": ["book", "pen"]}, "noise": True}),
        "result-path-merges": ({
            "StartAt": "Stamp", "States": {"Stamp": {
                "Type": "Pass", "Result": {"ok": True}, "ResultPath": "$.check", "End": True}},
        }, {"kept": 1}),
        "choice": ({
            "StartAt": "Size", "States": {
                "Size": {"Type": "Choice", "Choices": [
                    {"And": [{"Variable": "$.total", "NumericGreaterThanEquals": 100},
                             {"Variable": "$.tier", "StringEquals": "gold"}], "Next": "Big"},
                    {"Variable": "$.total", "NumericLessThan": 10, "Next": "Small"},
                    {"Not": {"Variable": "$.coupon", "IsPresent": True}, "Next": "Plain"}],
                    "Default": "Other"},
                "Big": {"Type": "Pass", "Result": "big", "End": True},
                "Small": {"Type": "Pass", "Result": "small", "End": True},
                "Plain": {"Type": "Pass", "Result": "plain", "End": True},
                "Other": {"Type": "Pass", "Result": "other", "End": True}},
        }, {"total": 250, "tier": "gold"}),
        "intrinsics": ({
            "StartAt": "Compute", "States": {"Compute": {"Type": "Pass", "End": True, "Parameters": {
                "format.$": "States.Format('{}-{}', $.a, $.b)",
                "array.$": "States.Array($.a, $.b, 3)",
                "add.$": "States.MathAdd($.n, 5)",
                "split.$": "States.StringSplit($.csv, ',')",
                "to_string.$": "States.JsonToString($.obj)",
                "to_json.$": "States.StringToJson($.text)",
                "contains.$": "States.ArrayContains($.list, 2)",
                "range.$": "States.ArrayRange(1, 9, 3)",
                "partition.$": "States.ArrayPartition($.list, 2)",
                "unique.$": "States.ArrayUnique($.dupes)",
                "item.$": "States.ArrayGetItem($.list, 1)",
                "b64.$": "States.Base64Encode($.a)",
                "hash.$": "States.Hash($.a, 'SHA-256')",
                "merge.$": "States.JsonMerge($.obj, $.other, false)"}}},
        }, {"a": "x", "b": "y", "n": 37, "csv": "p,q,r", "obj": {"k": 1}, "other": {"j": 2},
            "text": "{\"z\": [1, 2]}", "list": [1, 2, 3], "dupes": [1, 1, 2]}),
        "map": ({
            "StartAt": "Each", "States": {"Each": {
                "Type": "Map", "ItemsPath": "$.items", "MaxConcurrency": 1,
                "ItemSelector": {"value.$": "$$.Map.Item.Value", "index.$": "$$.Map.Item.Index"},
                "ItemProcessor": {"ProcessorConfig": {"Mode": "INLINE"}, "StartAt": "Tag", "States": {
                    "Tag": {"Type": "Pass", "Parameters": {"seen.$": "$.value", "at.$": "$.index"}, "End": True}}},
                "End": True}},
        }, {"items": ["a", "b", "c"]}),
        "parallel": ({
            "StartAt": "Both", "States": {"Both": {"Type": "Parallel", "End": True, "Branches": [
                {"StartAt": "L", "States": {"L": {"Type": "Pass", "Result": "left", "End": True}}},
                {"StartAt": "R", "States": {"R": {"Type": "Pass", "Parameters": {"got.$": "$.v"}, "End": True}}}]}},
        }, {"v": 1}),
        "fail": ({
            "StartAt": "Stop", "States": {"Stop": {"Type": "Fail", "Error": "Order.Invalid", "Cause": "no items"}},
        }, {}),
        "runtime-error": ({
            "StartAt": "Reach", "States": {"Reach": {
                "Type": "Pass", "Parameters": {"x.$": "$.not.there"}, "End": True}},
        }, {}),
        "wait-and-succeed": ({
            "StartAt": "Pause", "States": {
                "Pause": {"Type": "Wait", "Seconds": 1, "Next": "Done"}, "Done": {"Type": "Succeed"}},
        }, {"carried": True}),
    }
    for label, (definition, payload) in machines.items():
        arn, _ = machine(sfn, names, cleanup, role, definition, label=label)
        _, done = run(sfn, names, eventually, arn, payload, label=f"run-{label}")
        snapshot.match(label, {k: done.get(k) for k in ("status", "output", "error", "cause")})


def test_refusals(client, names, cleanup, snapshot, service_role, target):
    sfn = client("stepfunctions")
    role = service_role("states")
    good = {"StartAt": "A", "States": {"A": {"Type": "Succeed"}}}
    make = lambda label, definition, **kw: lambda: sfn.create_state_machine(
        name=names(label), definition=json.dumps(definition), roleArn=role, **kw)

    snapshot.error("no-start", make("bad", {"States": {"A": {"Type": "Succeed"}}}))
    snapshot.error("start-names-no-state", make("bad", {"StartAt": "Z", "States": {"A": {"Type": "Succeed"}}}))
    # A machine that can never end. The linter calls it an error; whether
    # CreateStateMachine does is recorded rather than assumed.
    cleanup(lambda: sfn.delete_state_machine(
        stateMachineArn=f"arn:aws:states:{target.region}:{target.account}:stateMachine:{names('loop')}"))
    snapshot.outcome("no-terminal-state", make("loop", {
        "StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "A"}}}), opaque=MINTED)
    snapshot.error("unknown-state-type", make("bad", {"StartAt": "A", "States": {"A": {"Type": "Shrug"}}}))
    snapshot.error("next-names-no-state", make("bad", {
        "StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "Nowhere"}}}))
    snapshot.error("definition-not-json", lambda: sfn.create_state_machine(
        name=names("bad"), definition="{nope", roleArn=role))
    snapshot.error("role-is-not-an-arn", lambda: sfn.create_state_machine(
        name=names("bad"), definition=json.dumps(good), roleArn="not-an-arn"))
    snapshot.error("bad-name", lambda: sfn.create_state_machine(
        name="has space", definition=json.dumps(good), roleArn=role))

    arn, _ = machine(sfn, names, cleanup, role, good)
    # The same name and the same definition is the same machine; a different
    # definition under that name is a conflict.
    snapshot.outcome("create-again-same", make("machine", good))
    snapshot.error("create-again-different", make(
        "machine", {"StartAt": "B", "States": {"B": {"Type": "Succeed"}}}))

    absent = f"arn:aws:states:{target.region}:{target.account}:stateMachine:{names('absent')}"
    snapshot.error("start-absent-machine", lambda: sfn.start_execution(stateMachineArn=absent))
    snapshot.error("describe-absent-machine", lambda: sfn.describe_state_machine(stateMachineArn=absent))
    snapshot.error("input-not-json", lambda: sfn.start_execution(stateMachineArn=arn, input="{nope"))
    snapshot.error("describe-absent-execution", lambda: sfn.describe_execution(
        executionArn=absent.replace(":stateMachine:", ":execution:") + ":never-ran"))

    run_name = names("run")
    sfn.start_execution(stateMachineArn=arn, name=run_name, input='{"a": 1}')
    snapshot.outcome("same-name-same-input", lambda: sfn.start_execution(
        stateMachineArn=arn, name=run_name, input='{"a": 1}'), opaque=MINTED)
    snapshot.error("same-name-different-input", lambda: sfn.start_execution(
        stateMachineArn=arn, name=run_name, input='{"a": 2}'))


# Written ahead of a recording (2026-11-13, A14): these hold doze-aws to what it answers today and score as UNVERIFIED until they
# are recorded. Each one is meant to be correct against AWS first; what a recording contradicts is fixed in doze-aws, not here.

def sync_client(client, target):
    """A Step Functions client for StartSyncExecution and TestState. The SDK sends those to `sync-<host>`; against doze-aws, whose
    endpoint is an IP address, that host prefix is turned off (a fresh client, so the shared one keeps it)."""
    sfn = client("stepfunctions", verify=True)
    if target.name == "doze":
        sfn.meta.config.inject_host_prefix = False
    return sfn


def history_types(sfn, execution_arn):
    events = sfn.get_execution_history(executionArn=execution_arn, maxResults=1000)["events"]
    return [e["type"] for e in events]


def test_versions_and_aliases(client, names, cleanup, snapshot, service_role, eventually):
    sfn = client("stepfunctions")
    one = {"StartAt": "A", "States": {"A": {"Type": "Pass", "Result": "one", "End": True}}}
    two = {"StartAt": "A", "States": {"A": {"Type": "Pass", "Result": "two", "End": True}}}
    arn, made = machine(sfn, names, cleanup, service_role("states"), one, publish=True, versionDescription="first")
    snapshot.match("create-published", made, opaque=MINTED)
    v1 = made["stateMachineVersionArn"]
    sfn.update_state_machine(stateMachineArn=arn, definition=json.dumps(two))
    v2 = snapshot.match("publish", sfn.publish_state_machine_version(stateMachineArn=arn, description="second"),
                        opaque=MINTED)["stateMachineVersionArn"]
    described = sfn.describe_state_machine(stateMachineArn=v1)
    described["definition"] = json.loads(described["definition"])
    snapshot.match("describe-version", described, opaque=MINTED, drop=("revisionId",))
    snapshot.match("list-versions", sfn.list_state_machine_versions(stateMachineArn=arn), opaque=MINTED)
    alias = sfn.create_state_machine_alias(
        name=names("alias"), routingConfiguration=[{"stateMachineVersionArn": v1, "weight": 100}])
    snapshot.match("create-alias", alias, opaque=MINTED)
    alias_arn = alias["stateMachineAliasArn"]
    snapshot.match("describe-alias", sfn.describe_state_machine_alias(stateMachineAliasArn=alias_arn), opaque=MINTED)
    started, done = run(sfn, names, eventually, alias_arn, {"via": "alias"})
    snapshot.match("run-through-alias", done, opaque=MINTED,
                   drop=("redriveCount", "redriveStatus", "redriveStatusReason"))
    snapshot.error("delete-routed-version", lambda: sfn.delete_state_machine_version(stateMachineVersionArn=v1))
    snapshot.error("weights-not-100", lambda: sfn.update_state_machine_alias(
        stateMachineAliasArn=alias_arn, routingConfiguration=[{"stateMachineVersionArn": v2, "weight": 50}]))
    snapshot.match("update-alias", sfn.update_state_machine_alias(
        stateMachineAliasArn=alias_arn, routingConfiguration=[{"stateMachineVersionArn": v2, "weight": 100}]),
        opaque=MINTED)
    snapshot.match("delete-alias", sfn.delete_state_machine_alias(stateMachineAliasArn=alias_arn))


def test_an_activity_worker(client, names, cleanup, snapshot, service_role, eventually):
    sfn = client("stepfunctions")
    made = sfn.create_activity(name=names("activity"))
    cleanup(sfn.delete_activity, activityArn=made["activityArn"])
    snapshot.match("create-activity", made, opaque=MINTED)
    act = made["activityArn"]
    snapshot.match("describe-activity", sfn.describe_activity(activityArn=act), opaque=MINTED)
    definition = {"StartAt": "Work", "States": {"Work": {"Type": "Task", "Resource": act, "TimeoutSeconds": 60, "End": True}}}
    arn, _ = machine(sfn, names, cleanup, service_role("states"), definition)
    started = sfn.start_execution(stateMachineArn=arn, name=names("run"), input='{"job": 7}')
    task = sfn.get_activity_task(activityArn=act, workerName="worker-1")
    snapshot.match("task-input", json.loads(task["input"]))
    sfn.send_task_heartbeat(taskToken=task["taskToken"])
    sfn.send_task_success(taskToken=task["taskToken"], output='{"done": 7}')

    def finished():
        d = sfn.describe_execution(executionArn=started["executionArn"])
        assert d["status"] != "RUNNING"
        return d

    done = eventually(finished, timeout=60)
    snapshot.match("activity-output", {"status": done["status"], "output": json.loads(done["output"])})
    snapshot.match("activity-history", history_types(sfn, started["executionArn"]))
    snapshot.error("token-spent", lambda: sfn.send_task_success(taskToken=task["taskToken"], output="{}"))


def test_a_task_token_and_a_caught_failure(client, names, cleanup, snapshot, service_role, eventually, target):
    sfn, sqs = client("stepfunctions"), client("sqs")
    url = sqs.create_queue(QueueName=names("queue"))["QueueUrl"]
    cleanup(sqs.delete_queue, QueueUrl=url)
    queue_arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
    absent = url.rsplit("/", 1)[0] + "/" + names("absent-queue")
    role = service_role("states", inline={"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Action": "sqs:SendMessage", "Resource": [queue_arn, f"arn:aws:sqs:{target.region}:*:*"]}]})
    definition = {"StartAt": "Missing", "States": {
        "Missing": {"Type": "Task", "Resource": "arn:aws:states:::sqs:sendMessage",
                    "Parameters": {"QueueUrl": absent, "MessageBody": "never"},
                    "Catch": [{"ErrorEquals": ["States.ALL"], "ResultPath": "$.caught", "Next": "Ask"}], "End": True},
        "Ask": {"Type": "Task", "Resource": "arn:aws:states:::sqs:sendMessage.waitForTaskToken",
                "Parameters": {"QueueUrl": url, "MessageBody": {"token.$": "$$.Task.Token"}},
                "ResultPath": "$.answer", "End": True}}}
    arn, _ = machine(sfn, names, cleanup, role, definition)
    started = sfn.start_execution(stateMachineArn=arn, name=names("run"), input="{}")

    def message():
        got = sqs.receive_message(QueueUrl=url, MaxNumberOfMessages=1, WaitTimeSeconds=1).get("Messages", [])
        assert got, "no message yet"
        return json.loads(got[0]["Body"])

    token = eventually(message, timeout=60)["token"]
    sfn.send_task_success(taskToken=token, output='{"approved": true}')

    def finished():
        d = sfn.describe_execution(executionArn=started["executionArn"])
        assert d["status"] != "RUNNING"
        return d

    done = eventually(finished, timeout=60)
    out = json.loads(done["output"])
    snapshot.match("caught-error-name", out["caught"]["Error"])
    snapshot.match("token-answer", out["answer"])
    snapshot.match("token-history", history_types(sfn, started["executionArn"]))


def test_express_sync_and_test_state(client, names, cleanup, snapshot, service_role, target):
    sfn = sync_client(client, target)
    definition = {"StartAt": "Shape", "States": {"Shape": {"Type": "Pass", "Parameters": {"sum.$": "States.MathAdd($.a, $.b)"},
                                                            "End": True}}}
    arn, _ = machine(client("stepfunctions"), names, cleanup, service_role("states"), definition, type="EXPRESS")
    done = sfn.start_sync_execution(stateMachineArn=arn, name=names("run"), input='{"a": 2, "b": 3}')
    done["output"] = json.loads(done["output"])
    done["input"] = json.loads(done["input"])
    snapshot.match("sync", done, opaque=MINTED + ("billingDetails", "executionArn"))
    failing = {"StartAt": "No", "States": {"No": {"Type": "Fail", "Error": "Nope", "Cause": "because"}}}
    bad_arn, _ = machine(client("stepfunctions"), names, cleanup, service_role("states"), failing, label="failing",
                         type="EXPRESS")
    failed = sfn.start_sync_execution(stateMachineArn=bad_arn, input="{}")
    snapshot.match("sync-failed", {k: failed.get(k) for k in ("status", "error", "cause")})
    choice = {"Type": "Choice", "Choices": [{"Variable": "$.n", "NumericGreaterThan": 1, "Next": "Big"}], "Default": "Small"}
    tested = sfn.test_state(definition=json.dumps(choice), input='{"n": 5}')
    snapshot.match("test-choice", {k: tested.get(k) for k in ("status", "nextState", "output")})
    pass_state = {"Type": "Pass", "InputPath": "$.a", "Parameters": {"x.$": "$.b"}, "ResultPath": "$.r", "Next": "After"}
    tested = sfn.test_state(definition=json.dumps(pass_state), input='{"a": {"b": 1}}', inspectionLevel="DEBUG")
    data = {k: json.loads(v) for k, v in tested.get("inspectionData", {}).items() if isinstance(v, str)}
    snapshot.match("test-pass", {"status": tested["status"], "nextState": tested.get("nextState"),
                                 "output": json.loads(tested["output"]), "inspection": data})


def test_validation_diagnostics(client, snapshot):
    sfn = client("stepfunctions")
    cases = {
        "valid": {"StartAt": "A", "States": {"A": {"Type": "Succeed"}}},
        "next-and-end": {"StartAt": "A", "States": {"A": {"Type": "Pass", "Next": "B", "End": True}, "B": {"Type": "Succeed"}}},
        "neither-next-nor-end": {"StartAt": "A", "States": {"A": {"Type": "Pass"}}},
        "unknown-field": {"StartAt": "A", "States": {"A": {"Type": "Pass", "Nope": 1, "End": True}}},
        "bad-result-path": {"StartAt": "A", "States": {"A": {"Type": "Pass", "ResultPath": "$.a[*]", "End": True}}},
        "all-not-last": {"StartAt": "A", "States": {"A": {"Type": "Task", "Resource": "arn:aws:states:::lambda:invoke",
                         "Retry": [{"ErrorEquals": ["States.ALL"]}, {"ErrorEquals": ["X"]}], "End": True}}},
        "wait-without-time": {"StartAt": "A", "States": {"A": {"Type": "Wait", "End": True}}},
        "missing-type": {"StartAt": "A", "States": {"A": {"End": True}}},
    }
    for label, definition in cases.items():
        snapshot.match(f"validate-{label}", sfn.validate_state_machine_definition(definition=json.dumps(definition)))


def test_jsonata_and_variables(client, names, cleanup, snapshot, service_role, eventually):
    sfn = client("stepfunctions")
    definition = {"QueryLanguage": "JSONata", "StartAt": "Count", "States": {
        "Count": {"Type": "Pass", "Assign": {"total": "{% $sum($states.input.items.price) %}"},
                  "Output": {"n": "{% $count($states.input.items) %}"}, "Next": "Decide"},
        "Decide": {"Type": "Choice", "Choices": [{"Condition": "{% $total > 10 %}", "Next": "Big"}], "Default": "Small"},
        "Big": {"Type": "Pass", "Output": {"size": "big", "total": "{% $total %}", "n": "{% $states.input.n %}"}, "End": True},
        "Small": {"Type": "Pass", "Output": {"size": "small"}, "End": True}}}
    arn, _ = machine(sfn, names, cleanup, service_role("states"), definition)
    _, done = run(sfn, names, eventually, arn, {"items": [{"price": 4}, {"price": 9}]})
    snapshot.match("jsonata", {k: done.get(k) for k in ("status", "output", "error", "cause")})
    variables = {"StartAt": "Set", "States": {
        "Set": {"Type": "Pass", "Assign": {"greeting.$": "$.hello"}, "Next": "Use"},
        "Use": {"Type": "Pass", "Parameters": {"said.$": "$greeting"}, "End": True}}}
    arn, _ = machine(sfn, names, cleanup, service_role("states"), variables, label="variables")
    _, done = run(sfn, names, eventually, arn, {"hello": "there"})
    snapshot.match("jsonpath-variables", {k: done.get(k) for k in ("status", "output", "error", "cause")})


def test_stop_tags_and_a_map_run(client, names, cleanup, snapshot, service_role, eventually):
    sfn = client("stepfunctions")
    waiting = {"StartAt": "W", "States": {"W": {"Type": "Wait", "Seconds": 300, "End": True}}}
    arn, _ = machine(sfn, names, cleanup, service_role("states"), waiting, tags=[{"key": "team", "value": "a"}])
    snapshot.match("tags", sfn.list_tags_for_resource(resourceArn=arn))
    sfn.tag_resource(resourceArn=arn, tags=[{"key": "env", "value": "dev"}])
    sfn.untag_resource(resourceArn=arn, tagKeys=["team"])
    snapshot.match("tags-after", sfn.list_tags_for_resource(resourceArn=arn))
    started = sfn.start_execution(stateMachineArn=arn, name=names("run"), input="{}")
    snapshot.match("stop", sfn.stop_execution(executionArn=started["executionArn"], error="Halt", cause="by test"),
                   opaque=MINTED)
    stopped = sfn.describe_execution(executionArn=started["executionArn"])
    snapshot.match("stopped", {k: stopped.get(k) for k in ("status", "error", "cause", "redriveStatus")})
    snapshot.match("stopped-history", history_types(sfn, started["executionArn"]))
    mapped = {"StartAt": "M", "States": {"M": {"Type": "Map", "MaxConcurrency": 2, "End": True,
              "ItemProcessor": {"ProcessorConfig": {"Mode": "DISTRIBUTED", "ExecutionType": "STANDARD"},
                                "StartAt": "P", "States": {"P": {"Type": "Pass", "End": True}}}}}}
    role = service_role("states", inline={"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Action": ["states:StartExecution", "states:DescribeExecution", "states:StopExecution"],
         "Resource": "*"}]})
    map_arn, _ = machine(sfn, names, cleanup, role, mapped, label="map")
    run_started, done = run(sfn, names, eventually, map_arn, [1, 2, 3])
    snapshot.match("map-output", {k: done.get(k) for k in ("status", "output")})
    runs = sfn.list_map_runs(executionArn=run_started["executionArn"])["mapRuns"]
    described = sfn.describe_map_run(mapRunArn=runs[0]["mapRunArn"])
    snapshot.match("map-run", {k: described.get(k) for k in ("status", "maxConcurrency", "itemCounts")})
