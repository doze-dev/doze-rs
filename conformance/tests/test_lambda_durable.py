"""Lambda durable functions through boto3: an execution that checkpoints a
step, waits, and waits for a callback the test answers.

The function speaks the durable execution protocol itself (the checkpoint and
replay calls the durable SDKs make), so it needs no SDK in its package. On AWS
it calls Lambda with the runtime's boto3; on doze-aws (which sets
AWS_ENDPOINT_URL for its functions) with the standard library, because the
host's python3 may have no boto3.
"""

from harness.helpers import zip_bytes

DURABLE = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicDurableExecutionRolePolicy"
FAST = {"Delay": 1, "MaxAttempts": 120}

HANDLER = '''
import datetime, json, os, urllib.parse, urllib.request

def local(method, path, body):
    now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    req = urllib.request.Request(os.environ["AWS_ENDPOINT_URL"] + path, method=method, data=json.dumps(body).encode())
    req.add_header("Content-Type", "application/json")
    req.add_header("X-Amz-Date", now)
    scope = f"{os.environ['AWS_ACCESS_KEY_ID']}/{now[:8]}/{os.environ['AWS_REGION']}/lambda/aws4_request"
    req.add_header("Authorization", f"AWS4-HMAC-SHA256 Credential={scope}, SignedHeaders=host;x-amz-date, Signature={'0' * 64}")
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read() or b"{}")

def checkpoint(arn, token, updates):
    if os.environ.get("AWS_ENDPOINT_URL"):
        path = "/2025-12-01/durable-executions/" + urllib.parse.quote(arn, safe="") + "/checkpoint"
        return local("POST", path, {"CheckpointToken": token, "Updates": updates})
    import boto3
    return boto3.client("lambda").checkpoint_durable_execution(
        DurableExecutionArn=arn, CheckpointToken=token, Updates=updates)

def handler(event, context):
    arn, token = event["DurableExecutionArn"], event["CheckpointToken"]
    ops = {o["Id"]: o for o in event["InitialExecutionState"]["Operations"]}

    def save(updates):
        nonlocal token
        out = checkpoint(arn, token, updates)
        token = out.get("CheckpointToken")
        for o in out["NewExecutionState"].get("Operations", []):
            ops[o["Id"]] = o

    execution = next(o for o in ops.values() if o["Type"] == "EXECUTION")
    given = json.loads(execution["ExecutionDetails"]["InputPayload"])
    if "step1" not in ops:
        save([{"Id": "step1", "Type": "STEP", "Action": "START", "Name": "double", "SubType": "Step"},
              {"Id": "step1", "Type": "STEP", "Action": "SUCCEED", "Payload": json.dumps(given["n"] * 2)}])
    doubled = json.loads(ops["step1"]["StepDetails"]["Result"])
    if "wait1" not in ops:
        save([{"Id": "wait1", "Type": "WAIT", "Action": "START", "Name": "pause", "WaitOptions": {"WaitSeconds": 2}}])
    if ops["wait1"]["Status"] != "SUCCEEDED":
        return {"Status": "PENDING"}
    if "cb1" not in ops:
        save([{"Id": "cb1", "Type": "CALLBACK", "Action": "START", "Name": "approve",
               "CallbackOptions": {"TimeoutSeconds": 120}}])
    cb = ops["cb1"]
    if cb["Status"] == "STARTED":
        return {"Status": "PENDING"}
    if cb["Status"] != "SUCCEEDED":
        return {"Status": "FAILED", "Error": cb["CallbackDetails"].get("Error", {})}
    return {"Status": "SUCCEEDED", "Result": json.dumps({"doubled": doubled, "approval": cb["CallbackDetails"]["Result"]})}
'''

# Clock readings, generated names and ids, and where AWS keeps the code: not ours to match.
MINTED = ("LastModified", "Location", "RepositoryType", "DurableExecutionArn", "FunctionArn", "StartTimestamp",
          "EndTimestamp", "EventTimestamp", "StopTimestamp", "CallbackId", "RequestId", "ScheduledEndTimestamp", "Id")
SETTLING = ("State", "StateReason", "StateReasonCode", "LastUpdateStatus",
            "LastUpdateStatusReason", "LastUpdateStatusReasonCode", "RuntimeVersionConfig")


def durable_function(lam, names, cleanup, role):
    name = names("durable")
    made = lam.create_function(
        FunctionName=name, Runtime="python3.13", Role=role, Handler="index.handler", Timeout=30,
        Code={"ZipFile": zip_bytes({"index.py": HANDLER})},
        DurableConfig={"ExecutionTimeout": 600, "RetentionPeriodInDays": 1})
    cleanup(lam.delete_function, FunctionName=name)
    lam.get_waiter("function_active_v2").wait(FunctionName=name, WaiterConfig=FAST)
    return name, made


def events(lam, arn):
    return lam.get_durable_execution_history(DurableExecutionArn=arn)["Events"]


def callback_of(lam, arn, eventually):
    def started():
        found = [e for e in events(lam, arn) if e["EventType"] == "CallbackStarted"]
        assert found, "the callback has not started"
        return found[0]["CallbackStartedDetails"]["CallbackId"]
    return eventually(started, timeout=120)


def ended(lam, arn, eventually):
    def done():
        got = lam.get_durable_execution(DurableExecutionArn=arn)
        assert got["Status"] != "RUNNING", "still running"
        return got
    return eventually(done, timeout=120)


def test_durable_execution_with_a_wait_and_a_callback(client, names, cleanup, snapshot, service_role, eventually):
    lam = client("lambda")
    name, made = durable_function(lam, names, cleanup, service_role("lambda", DURABLE))
    snapshot.match("create", made["DurableConfig"])
    snapshot.match("configuration", lam.get_function_configuration(FunctionName=name)["DurableConfig"])
    snapshot.error("invoke-unqualified", lambda: lam.invoke(FunctionName=name, Payload=b'{"n": 21}'))

    started = lam.invoke(FunctionName=name, Qualifier="$LATEST", InvocationType="Event",
                         DurableExecutionName="order-1", Payload=b'{"n": 21}')
    snapshot.match("invoke-event", started, opaque=MINTED, drop=("Payload",))
    arn = started["DurableExecutionArn"]
    again = lam.invoke(FunctionName=name, Qualifier="$LATEST", InvocationType="Event",
                       DurableExecutionName="order-1", Payload=b'{"n": 21}')
    assert again["DurableExecutionArn"] == arn
    # Recorded 2026-11-19: AWS accepts this (the scenario expected a refusal); what it answers is kept.
    snapshot.outcome("invoke-same-name-other-payload", lambda: (lambda r: {"StatusCode": r["StatusCode"],
                     "same-execution": r.get("DurableExecutionArn") == arn})(lam.invoke(
        FunctionName=name, Qualifier="$LATEST", InvocationType="Event",
        DurableExecutionName="order-1", Payload=b'{"n": 1}')))

    callback = callback_of(lam, arn, eventually)
    snapshot.match("running", lam.get_durable_execution(DurableExecutionArn=arn), opaque=MINTED)
    snapshot.match("heartbeat", lam.send_durable_execution_callback_heartbeat(CallbackId=callback))
    snapshot.match("succeed", lam.send_durable_execution_callback_success(CallbackId=callback, Result=b'"yes"'))
    snapshot.match("succeeded", ended(lam, arn, eventually), opaque=MINTED)
    snapshot.error("succeed-again", lambda: lam.send_durable_execution_callback_success(CallbackId=callback))
    snapshot.match("history", [e["EventType"] for e in events(lam, arn)])
    snapshot.match("history-newest-first", lam.get_durable_execution_history(
        DurableExecutionArn=arn, ReverseOrder=True, MaxItems=2, IncludeExecutionData=False), opaque=MINTED,
        drop=("NextMarker", "EventId"))
    snapshot.match("list", lam.list_durable_executions_by_function(
        FunctionName=name, Statuses=["SUCCEEDED"]), opaque=MINTED)
    snapshot.error("state-with-a-made-up-token", lambda: lam.get_durable_execution_state(
        DurableExecutionArn=arn, CheckpointToken="bm90LWEtdG9rZW4="))


def test_durable_execution_stopped(client, names, cleanup, snapshot, service_role, eventually):
    lam = client("lambda")
    name, _ = durable_function(lam, names, cleanup, service_role("lambda", DURABLE))
    started = lam.invoke(FunctionName=name, Qualifier="$LATEST", InvocationType="Event", Payload=b'{"n": 2}')
    arn = started["DurableExecutionArn"]
    callback = callback_of(lam, arn, eventually)
    snapshot.match("stop", lam.stop_durable_execution(
        DurableExecutionArn=arn, Error={"ErrorMessage": "changed my mind", "ErrorType": "Cancelled"}), opaque=MINTED)
    snapshot.match("stopped", lam.get_durable_execution(DurableExecutionArn=arn), opaque=MINTED)
    # Recorded 2026-11-19: AWS accepts a second stop.
    snapshot.outcome("stop-again", lambda: lam.stop_durable_execution(DurableExecutionArn=arn), opaque=("StopTimestamp",))
    snapshot.error("fail-after-stop", lambda: lam.send_durable_execution_callback_failure(
        CallbackId=callback, Error={"ErrorMessage": "no"}))


def test_durability_is_chosen_at_creation(client, names, cleanup, snapshot, service_role):
    lam = client("lambda")
    role = service_role("lambda", DURABLE)
    name = names("plain")
    lam.create_function(FunctionName=name, Runtime="python3.13", Role=role, Handler="index.handler",
                        Code={"ZipFile": zip_bytes({"index.py": HANDLER})})
    cleanup(lam.delete_function, FunctionName=name)
    lam.get_waiter("function_active_v2").wait(FunctionName=name, WaiterConfig=FAST)
    snapshot.error("enable-later", lambda: lam.update_function_configuration(
        FunctionName=name, DurableConfig={"ExecutionTimeout": 60}))
    snapshot.error("no-execution-timeout", lambda: lam.create_function(
        FunctionName=names("no-timeout"), Runtime="python3.13", Role=role, Handler="index.handler",
        Code={"ZipFile": zip_bytes({"index.py": HANDLER})}, DurableConfig={"RetentionPeriodInDays": 3}))
    snapshot.error("get-absent", lambda: lam.get_durable_execution(
        DurableExecutionArn=f"arn:aws:lambda:{lam.meta.region_name}:{role.split(':')[4]}:function:{name}:$LATEST"
                            "/durable-execution/absent/absent"))
