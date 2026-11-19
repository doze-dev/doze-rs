"""CloudWatch Logs Insights, Live Tail, export tasks, destinations, transformers, field indexes and scheduled queries through boto3.

Written ahead of a recording (2026-11-18, logs-ops): these hold doze-aws to what it answers today and score as UNVERIFIED until
they are recorded. Each is meant to be correct against AWS first; what a recording contradicts is fixed in doze-aws, not here.
Cost: a few hundred requests and kilobytes of logs; one S3 bucket with a few objects. Nothing bills by the hour.
"""

import json
import time

# Clock readings and the ids the service mints.
CLOCK = ("creationTime", "lastUpdateTime", "lastUpdatedTime", "createTime", "lastScanTime", "firstEventTime", "lastEventTime",
         "lastModifiedTime", "timestamp", "ingestionTime", "completionTime", "queryDuration")
MINTED = ("nextToken", "queryId", "sessionId", "requestId", "taskId")
# Shown on the service's own schedule.
WEATHER = ("statistics", "bytesScanned")


def group(logs, names, cleanup, label="group"):
    name = f"/{names(label)}"
    logs.create_log_group(logGroupName=name)
    cleanup(logs.delete_log_group, logGroupName=name)
    return name


def arn_of(logs, name):
    return logs.describe_log_groups(logGroupNamePrefix=name)["logGroups"][0]["logGroupArn"]


def put(logs, name, stream, lines):
    now = int(time.time() * 1000)
    logs.put_log_events(logGroupName=name, logStreamName=stream,
                        logEvents=[{"timestamp": now + i, "message": m} for i, m in enumerate(lines)])
    return now


LINES = ['{"level":"info","ms":10,"path":"/a"}', '{"level":"error","ms":250,"path":"/b"}',
         '{"level":"info","ms":30,"path":"/a"}', "plain ERROR user=bob action=login"]


def rows(result):
    """A result as dicts, without the pointer and the time (both are the service's to mint)."""
    return [{f["field"]: f["value"] for f in row if f["field"] not in ("@ptr", "@timestamp")} for row in result["results"]]


def run_query(logs, eventually, name, query, want, start):
    """Runs the query until it sees `want` rows. AWS (recorded 2026-11-19) can complete a query before events just put are
    searchable, and a completed query does not change, so a short answer starts the query again."""
    state = {}

    def done():
        if "qid" not in state:
            state["qid"] = logs.start_query(logGroupName=name, startTime=start // 1000 - 60, endTime=int(time.time()) + 60,
                                            queryString=query)["queryId"]
        r = logs.get_query_results(queryId=state["qid"])
        assert r["status"] == "Complete", r["status"]
        if len(r["results"]) < want:
            del state["qid"]
            raise AssertionError(f"{len(r['results'])} of {want} rows so far")
        return r
    r = eventually(done, timeout=300, every=2)
    return state["qid"], r


def test_insights_queries(client, names, cleanup, snapshot, eventually):
    logs = client("logs")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="app")
    start = put(logs, name, "app", LINES)

    qid, r = run_query(logs, eventually, name, "fields path, ms | filter level = 'error' or ms < 20 | sort ms desc", 2, start)
    snapshot.match("fields-filter-sort", rows(r))
    _, r = run_query(logs, eventually, name, "filter ispresent(level) | stats count(*) as n, avg(ms) as mean by level", 2, start)
    snapshot.match("stats-by", sorted(rows(r), key=lambda x: x["level"]))
    _, r = run_query(logs, eventually, name, "parse @message 'user=* action=*' as user, action | filter ispresent(user) "
                     "| fields toupper(user) as up | display user, action, up", 1, start)
    snapshot.match("parse-glob", rows(r))
    # AWS (recorded 2026-11-19): a bare `fields` naming a field `parse` made is refused.
    snapshot.error("parse-then-fields-again", lambda: logs.start_query(
        logGroupName=name, startTime=start // 1000 - 60, endTime=int(time.time()) + 60,
        queryString="parse @message 'user=* action=*' as user, action | fields user, action"))
    _, r = run_query(logs, eventually, name, "filter @message like /ERROR/ | fields @message, @logStream", 1, start)
    snapshot.match("like-regex", rows(r))
    _, r = run_query(logs, eventually, name, "fields @message | limit 2", 2, start)
    snapshot.match("limit-newest-first", rows(r))
    snapshot.match("result-shape", {k: v for k, v in r.items() if k != "ResponseMetadata"},
                   opaque=("value",), drop=WEATHER)

    ptr = next(f["value"] for f in r["results"][0] if f["field"] == "@ptr")
    snapshot.match("record", logs.get_log_record(logRecordPointer=ptr), opaque=("@timestamp", "@ingestionTime"))
    snapshot.match("described", logs.describe_queries(logGroupName=name, maxResults=1),
                   opaque=CLOCK + MINTED, drop=WEATHER)
    snapshot.match("groups-for-query", logs.list_log_groups_for_query(queryId=qid))
    snapshot.error("stop-ended", lambda: logs.stop_query(queryId=qid))
    snapshot.error("no-such-query", lambda: logs.get_query_results(queryId="00000000-0000-0000-0000-000000000000"))

    def fields():
        got = logs.get_log_group_fields(logGroupName=name)["logGroupFields"]
        assert any(f["name"] == "level" for f in got), got
        return got
    snapshot.match("group-fields", fields_of(eventually(fields, timeout=120, every=2)))


def fields_of(got):
    return sorted((f["name"], f["percent"]) for f in got)


def test_insights_refusals(client, names, cleanup, snapshot):
    logs = client("logs")
    name = group(logs, names, cleanup)
    now = int(time.time())
    snapshot.error("no-group", lambda: logs.start_query(startTime=now - 60, endTime=now, queryString="fields @message"))
    snapshot.error("two-ways", lambda: logs.start_query(logGroupName=name, logGroupNames=[name], startTime=now - 60,
                                                        endTime=now, queryString="fields @message"))
    snapshot.error("absent-group", lambda: logs.start_query(logGroupName=f"/{names('absent')}", startTime=now - 60,
                                                            endTime=now, queryString="fields @message"))
    snapshot.error("backwards", lambda: logs.start_query(logGroupName=name, startTime=now, endTime=now - 60,
                                                         queryString="fields @message"))
    snapshot.error("malformed", lambda: logs.start_query(logGroupName=name, startTime=now - 60, endTime=now,
                                                         queryString="fields | |"))
    snapshot.error("unknown-function", lambda: logs.start_query(logGroupName=name, startTime=now - 60, endTime=now,
                                                                queryString="fields nosuch(@message)"))
    snapshot.error("bad-pointer", lambda: logs.get_log_record(logRecordPointer="not-a-pointer"))


def test_live_tail(client, names, cleanup, snapshot, target):
    logs = client("logs", verify=True)
    if target.name == "doze":
        logs.meta.config.inject_host_prefix = False
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="app")
    arn = arn_of(logs, name)
    stream = logs.start_live_tail(logGroupIdentifiers=[arn], logEventFilterPattern="ERROR")["responseStream"]
    seen, start = [], None
    deadline = time.time() + 60
    for event in stream:
        if "sessionStart" in event:
            start = event["sessionStart"]
            put(logs, name, "app", ["INFO fine", "ERROR one", "ERROR two"])
        elif "sessionUpdate" in event:
            seen += [e["message"] for e in event["sessionUpdate"]["sessionResults"]]
        if len(seen) >= 2 or time.time() > deadline:
            break
    stream.close()
    snapshot.match("session-start", start, opaque=MINTED)
    snapshot.match("tailed", seen)
    snapshot.error("names-a-stream-and-a-prefix", lambda: logs.start_live_tail(
        logGroupIdentifiers=[arn], logStreamNames=["app"], logStreamNamePrefixes=["a"]))


def test_export_task(client, names, cleanup, snapshot, eventually, target):
    logs, s3 = client("logs"), client("s3")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="app")
    start = put(logs, name, "app", ["first", "second"])
    bucket = names("bucket")
    s3.create_bucket(Bucket=bucket)
    cleanup(s3.delete_bucket, Bucket=bucket)
    s3.put_bucket_policy(Bucket=bucket, Policy=json.dumps({"Version": "2012-10-17", "Statement": [
        {"Effect": "Allow", "Principal": {"Service": "logs.amazonaws.com"}, "Action": "s3:GetBucketAcl",
         "Resource": f"arn:aws:s3:::{bucket}"},
        {"Effect": "Allow", "Principal": {"Service": "logs.amazonaws.com"}, "Action": "s3:PutObject",
         "Resource": f"arn:aws:s3:::{bucket}/*"}]}))
    task = logs.create_export_task(logGroupName=name, fromTime=start - 1000, to=start + 60_000, destination=bucket,
                                   destinationPrefix="out")["taskId"]

    def finished():
        t = logs.describe_export_tasks(taskId=task)["exportTasks"][0]
        assert t["status"]["code"] == "COMPLETED", t["status"]
        return t
    snapshot.match("completed", eventually(finished, timeout=300, every=5), opaque=CLOCK + MINTED + ("from", "to"))
    keys = sorted(o["Key"] for o in s3.list_objects_v2(Bucket=bucket).get("Contents", []))
    for k in keys:
        cleanup(s3.delete_object, Bucket=bucket, Key=k)
    snapshot.match("objects", [k.replace(task, "<task>") for k in keys])
    snapshot.error("cancel-finished", lambda: logs.cancel_export_task(taskId=task))
    snapshot.error("absent-group", lambda: logs.create_export_task(
        logGroupName=f"/{names('absent')}", fromTime=0, to=1, destination=bucket))


def test_destinations(client, names, cleanup, snapshot, service_role):
    logs = client("logs")
    role = service_role("logs")
    dest = names("dest")
    # A stream that is never made: a Kinesis stream bills by the hour. Whether AWS takes a destination whose stream is absent is
    # the question `outcome` holds.
    account = client("sts").get_caller_identity()["Account"]
    target_arn = f"arn:aws:kinesis:{logs.meta.region_name}:{account}:stream/{names('stream')}"
    made = snapshot.outcome("put", lambda: logs.put_destination(destinationName=dest, targetArn=target_arn, roleArn=role),
                            opaque=CLOCK)
    cleanup(logs.delete_destination, destinationName=dest)
    if isinstance(made, dict):
        policy = json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"AWS": "111122223333"},
                             "Action": "logs:PutSubscriptionFilter", "Resource": made["destination"]["arn"]}]})
        snapshot.match("policy", logs.put_destination_policy(destinationName=dest, accessPolicy=policy))
        snapshot.match("described", logs.describe_destinations(DestinationNamePrefix=dest), opaque=CLOCK)
        snapshot.match("delete", logs.delete_destination(destinationName=dest))
    snapshot.error("delete-absent", lambda: logs.delete_destination(destinationName=names("absent")))


def test_transformers(client, names, cleanup, snapshot, eventually):
    logs = client("logs")
    name = group(logs, names, cleanup)
    config = [{"parseJSON": {}}, {"addKeys": {"entries": [{"key": "env", "value": "dev"}]}},
              {"renameKeys": {"entries": [{"key": "lvl", "renameTo": "level"}]}},
              {"upperCaseString": {"withKeys": ["level"]}}]
    snapshot.match("put", logs.put_transformer(logGroupIdentifier=name, transformerConfig=config))
    snapshot.match("get", logs.get_transformer(logGroupIdentifier=name), opaque=CLOCK)
    tests = {
        "json": (config, ['{"lvl":"warn","n":1}']),
        "grok": ([{"grok": {"match": "%{IP:client} %{WORD:method} %{URIPATHPARAM:request} %{NUMBER:bytes}"}}],
                 ["55.3.244.1 GET /index.html?x=1 15824"]),
        "csv": ([{"csv": {"columns": ["a", "b", "c"]}}], ['1,"two, too",3']),
        "key-value": ([{"parseKeyValue": {"fieldDelimiter": " ", "keyValueDelimiter": "="}},
                       {"typeConverter": {"entries": [{"key": "n", "type": "integer"}]}}], ["a=1 b=x n=7"]),
        "strings": ([{"parseJSON": {}}, {"trimString": {"withKeys": ["s"]}}, {"lowerCaseString": {"withKeys": ["u"]}},
                     {"splitString": {"entries": [{"source": "list", "delimiter": ","}]}},
                     {"substituteString": {"entries": [{"source": "u", "from": "o+", "to": "0"}]}}],
                    ['{"s":"  pad  ","u":"FOO","list":"a,b,c"}']),
        "keys": ([{"parseJSON": {}}, {"copyValue": {"entries": [{"source": "a", "target": "b"}]}},
                  {"deleteKeys": {"withKeys": ["x"]}}],
                 ['{"a":1,"c":2,"x":3}']),
        "date": ([{"parseJSON": {}}, {"dateTimeConverter": {"source": "t", "target": "iso",
                                                             "matchPatterns": ["dd/MMM/yyyy:HH:mm:ss Z"]}}],
                 ['{"t":"10/Oct/2000:13:55:36 -0700"}']),
    }
    for label, (cfg, messages) in tests.items():
        got = logs.test_transformer(transformerConfig=cfg, logEventMessages=messages)["transformedLogs"]
        snapshot.match(f"test-{label}", [json.loads(t["transformedEventMessage"]) for t in got])
    # AWS (recorded 2026-11-19) refused moveKeys c -> d and c -> d.e ("No corresponding field. Field Name: 'd'"); which rule that is
    # (a target that must exist?) is what these three settle.
    def move(target, **kw):
        return lambda: [json.loads(t["transformedEventMessage"]) for t in logs.test_transformer(
            transformerConfig=[{"parseJSON": {}}, {"moveKeys": {"entries": [{"source": "c", "target": target, **kw}]}}],
            logEventMessages=['{"a":1,"c":2}'])["transformedLogs"]]
    snapshot.outcome("move-to-new", move("d"))
    snapshot.outcome("move-to-existing", move("a", overwriteIfExists=True))
    snapshot.outcome("move-under-absent", move("d.e"))
    # AWS (recorded 2026-11-19): a grok pattern with a type conversion is refused.
    snapshot.error("grok-type-conversion", lambda: logs.test_transformer(
        transformerConfig=[{"grok": {"match": "%{NUMBER:bytes:int}"}}], logEventMessages=["15824"]))
    snapshot.error("ocsf-not-first-or-unknown", lambda: logs.test_transformer(
        transformerConfig=[{"addKeys": {"entries": [{"key": "a", "value": "b"}]}}, {"parseVPC": {}}],
        logEventMessages=["x"]))
    snapshot.match("delete", logs.delete_transformer(logGroupIdentifier=name))
    # AWS (recorded 2026-11-19) still answered GetTransformer a minute after the delete; what it answers is kept.
    snapshot.outcome("get-deleted", lambda: logs.get_transformer(logGroupIdentifier=name), opaque=CLOCK)


def test_index_policies(client, names, cleanup, snapshot):
    logs = client("logs")
    name = group(logs, names, cleanup)
    doc = json.dumps({"Fields": ["requestId"], "FieldsV2": {"status": {"type": "FACET"}}})
    snapshot.match("put", logs.put_index_policy(logGroupIdentifier=name, policyDocument=doc), opaque=CLOCK)
    snapshot.match("describe", logs.describe_index_policies(logGroupIdentifiers=[name]), opaque=CLOCK)
    got = logs.describe_field_indexes(logGroupIdentifiers=[name], indexCategories=["CUSTOM"])["fieldIndexes"]
    snapshot.match("custom-indexes", sorted((i["fieldIndexName"], i["type"], i["indexCategory"]) for i in got))
    snapshot.error("empty-policy", lambda: logs.put_index_policy(logGroupIdentifier=name, policyDocument='{"Fields":[]}'))
    snapshot.match("delete", logs.delete_index_policy(logGroupIdentifier=name))
    snapshot.error("delete-again", lambda: logs.delete_index_policy(logGroupIdentifier=name))


def test_scheduled_queries(client, names, cleanup, snapshot, service_role):
    logs = client("logs")
    name = group(logs, names, cleanup)
    role = service_role("logs")
    sq = names("scheduled")
    made = logs.create_scheduled_query(name=sq, queryLanguage="CWLI", queryString="stats count(*) as n",
                                       logGroupIdentifiers=[name], scheduleExpression="cron(0 0 * * ? *)",
                                       startTimeOffset=3600, executionRoleArn=role, state="DISABLED")
    cleanup(logs.delete_scheduled_query, identifier=made["scheduledQueryArn"])
    snapshot.match("create", made)
    snapshot.match("get", logs.get_scheduled_query(identifier=sq), opaque=CLOCK)
    snapshot.match("updated", logs.update_scheduled_query(identifier=sq, queryLanguage="CWLI", queryString="fields @message",
                                                          scheduleExpression="cron(0 1 * * ? *)", executionRoleArn=role,
                                                          startTimeOffset=3600, logGroupIdentifiers=[name], state="DISABLED"), opaque=CLOCK)
    snapshot.error("update-without-offset", lambda: logs.update_scheduled_query(
        identifier=sq, queryLanguage="CWLI", queryString="fields @message", scheduleExpression="cron(0 1 * * ? *)",
        executionRoleArn=role, logGroupIdentifiers=[name], state="DISABLED"))
    listed = [q for q in logs.list_scheduled_queries()["scheduledQueries"] if q["name"] == sq]
    snapshot.match("listed", listed, opaque=CLOCK)
    now = int(time.time())
    snapshot.match("history", logs.get_scheduled_query_history(identifier=sq, startTime=now - 3600, endTime=now))
    snapshot.error("duplicate", lambda: logs.create_scheduled_query(
        name=sq, queryLanguage="CWLI", queryString="fields @message", scheduleExpression="cron(0 0 * * ? *)",
        executionRoleArn=role))
    snapshot.match("delete", logs.delete_scheduled_query(identifier=sq))
    snapshot.error("get-deleted", lambda: logs.get_scheduled_query(identifier=sq))
