"""CloudWatch Logs through boto3."""

import time

# Clock readings, in milliseconds since the epoch, wherever Logs reports one.
CLOCK = ("creationTime", "timestamp", "ingestionTime", "firstEventTimestamp",
         "lastEventTimestamp", "lastIngestionTime")
# Cursors and ids the service mints.
MINTED = ("nextToken", "nextForwardToken", "nextBackwardToken", "nextSequenceToken", "eventId",
          "uploadSequenceToken")
# Updated on the service's own schedule.
WEATHER = ("storedBytes",)


def group(logs, names, cleanup, label="group"):
    name = f"/{names(label)}"
    logs.create_log_group(logGroupName=name)
    cleanup(logs.delete_log_group, logGroupName=name)
    return name


def test_group_and_stream_lifecycle(client, names, cleanup, snapshot):
    logs = client("logs")
    name = f"/{names('group')}"
    snapshot.match("create-group", logs.create_log_group(
        logGroupName=name, tags={"env": "dev"}))
    cleanup(logs.delete_log_group, logGroupName=name)
    snapshot.match("describe", logs.describe_log_groups(logGroupNamePrefix=name),
                   opaque=CLOCK, drop=WEATHER)
    snapshot.match("set-retention", logs.put_retention_policy(logGroupName=name, retentionInDays=7))
    snapshot.match("with-retention", logs.describe_log_groups(logGroupNamePrefix=name),
                   opaque=CLOCK, drop=WEATHER)
    snapshot.match("drop-retention", logs.delete_retention_policy(logGroupName=name))
    snapshot.match("tags", logs.list_tags_log_group(logGroupName=name))

    snapshot.match("create-stream", logs.create_log_stream(logGroupName=name, logStreamName="app"))
    snapshot.match("streams", logs.describe_log_streams(logGroupName=name),
                   opaque=CLOCK + MINTED, drop=WEATHER)
    snapshot.match("delete-stream", logs.delete_log_stream(logGroupName=name, logStreamName="app"))
    snapshot.match("delete-group", logs.delete_log_group(logGroupName=name))


def test_events_are_written_read_and_filtered(client, names, cleanup, snapshot, eventually):
    logs = client("logs")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="app")
    now = int(time.time() * 1000)
    lines = ["INFO started", "ERROR disk full", '{"level":"error","code":507}', "INFO stopped"]
    snapshot.match("put", logs.put_log_events(
        logGroupName=name, logStreamName="app",
        logEvents=[{"timestamp": now + i, "message": m} for i, m in enumerate(lines)]),
        opaque=MINTED)

    def read():
        r = logs.get_log_events(logGroupName=name, logStreamName="app", startFromHead=True)
        assert len(r["events"]) == len(lines), f"{len(r['events'])} of {len(lines)} so far"
        return r

    snapshot.match("get", eventually(read, timeout=60), opaque=CLOCK + MINTED)

    def filtered(pattern, want):
        def poll():
            r = logs.filter_log_events(logGroupName=name, filterPattern=pattern)
            assert len(r["events"]) >= want, f"{len(r['events'])} of {want} so far"
            return [e["message"] for e in r["events"]]
        return eventually(poll, timeout=60)

    snapshot.match("filter-term", filtered("ERROR", 1))
    snapshot.match("filter-two-terms", filtered("INFO started", 1))
    snapshot.match("filter-quoted", filtered('"disk full"', 1))
    snapshot.match("filter-exclude", filtered("INFO -started", 1))
    snapshot.match("filter-json", filtered('{ $.code = 507 }', 1))
    snapshot.match("filter-json-string", filtered('{ $.level = "error" }', 1))


def test_refusals(client, names, cleanup, snapshot):
    logs = client("logs")
    absent = f"/{names('absent')}"
    snapshot.error("stream-in-absent-group", lambda: logs.create_log_stream(
        logGroupName=absent, logStreamName="app"))
    snapshot.error("delete-absent-group", lambda: logs.delete_log_group(logGroupName=absent))
    snapshot.error("events-from-absent-group", lambda: logs.get_log_events(
        logGroupName=absent, logStreamName="app"))

    name = group(logs, names, cleanup)
    snapshot.error("create-group-twice", lambda: logs.create_log_group(logGroupName=name))
    snapshot.error("retention-not-allowed", lambda: logs.put_retention_policy(
        logGroupName=name, retentionInDays=2))
    snapshot.error("events-from-absent-stream", lambda: logs.get_log_events(
        logGroupName=name, logStreamName="absent"))
    snapshot.error("put-to-absent-stream", lambda: logs.put_log_events(
        logGroupName=name, logStreamName="absent",
        logEvents=[{"timestamp": int(time.time() * 1000), "message": "x"}]))

    logs.create_log_stream(logGroupName=name, logStreamName="app")
    snapshot.error("create-stream-twice", lambda: logs.create_log_stream(
        logGroupName=name, logStreamName="app"))
    now = int(time.time() * 1000)
    snapshot.error("out-of-order", lambda: logs.put_log_events(
        logGroupName=name, logStreamName="app",
        logEvents=[{"timestamp": now, "message": "b"}, {"timestamp": now - 1000, "message": "a"}]))
    snapshot.error("bad-filter-pattern", lambda: logs.filter_log_events(
        logGroupName=name, filterPattern="{ $.code = }"))


# Scenarios below were written ahead of a recording (2026-11-12): against doze-aws they run and must pass, but every response is
# UNVERIFIED until they are recorded against AWS.

def test_filter_patterns_of_each_form(client, names, cleanup, snapshot, eventually):
    logs = client("logs")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="web")
    now = int(time.time() * 1000)
    lines = ['127.0.0.1 - frank [10/Oct/2000:13:55:36] "GET /a.gif HTTP/1.0" 404 2326',
             '127.0.0.1 - frank [10/Oct/2000:13:55:37] "GET /b.gif HTTP/1.0" 200 512',
             '{"level":"warn","ms":250,"route":"/slow"}', '{"level":"info","ms":5,"route":"/fast"}',
             "plain ERROR text", "plain WARN text"]
    logs.put_log_events(logGroupName=name, logStreamName="web", logEvents=[
        {"timestamp": now + i, "message": m} for i, m in enumerate(lines)])

    def pick(pattern, want):
        def poll():
            r = logs.filter_log_events(logGroupName=name, filterPattern=pattern)
            assert len(r["events"]) >= want, f"{len(r['events'])} of {want} so far"
            return [e["message"] for e in r["events"]]
        return eventually(poll, timeout=60)

    snapshot.match("space-delimited", pick("[ip, user, username, timestamp, request, status_code = 4*, bytes]", 1))
    snapshot.match("space-delimited-number", pick("[ip, user, username, timestamp, request, status_code, bytes > 1000]", 1))
    snapshot.match("json-and", pick('{ $.level = "warn" && $.ms > 100 }', 1))
    snapshot.match("json-or", pick('{ $.level = "info" || $.ms > 100 }', 2))
    snapshot.match("either-term", pick("?ERROR ?WARN", 2))
    snapshot.match("wildcard-json-string", pick('{ $.route = "/s*" }', 1))


def test_regular_expressions_in_filter_patterns(client, names, cleanup, snapshot, eventually):
    """`%...%` in each form of pattern, and the quota of two in one pattern (written ahead of the recording)."""
    logs = client("logs")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="web")
    now = int(time.time() * 1000)
    lines = ['10.0.0.1 PUT /items 201', '10.0.0.2 GET /items 503',
             '{"path":"/api/v2/users","code":503}', '{"path":"/health","code":200}',
             "timeout talking to db", "all fine"]
    logs.put_log_events(logGroupName=name, logStreamName="web", logEvents=[
        {"timestamp": now + i, "message": m} for i, m in enumerate(lines)])

    def pick(pattern, want):
        def poll():
            r = logs.filter_log_events(logGroupName=name, filterPattern=pattern)
            assert len(r["events"]) >= want, f"{len(r['events'])} of {want} so far"
            return [e["message"] for e in r["events"]]
        return eventually(poll, timeout=60)

    snapshot.match("regex-term", pick("%time.?out|refused%", 1))
    snapshot.match("regex-json", pick("{ $.path = %^/api/v[12]/% }", 1))
    snapshot.match("regex-space", pick(r"[ip, method = %GET|PUT%, path, status = %^5\d\d$%]", 1))
    snapshot.match("regex-test-metric-filter", logs.test_metric_filter(
        filterPattern=r"%5\d\d%", logEventMessages=["status 503", "status 200"]))
    snapshot.error("regex-three-in-one", lambda: logs.test_metric_filter(
        filterPattern="%a% %b% %c%", logEventMessages=["a b c"]))


def test_get_log_events_pages_in_both_directions(client, names, cleanup, snapshot, eventually):
    logs = client("logs")
    name = group(logs, names, cleanup)
    logs.create_log_stream(logGroupName=name, logStreamName="app")
    now = int(time.time() * 1000)
    logs.put_log_events(logGroupName=name, logStreamName="app", logEvents=[
        {"timestamp": now + i, "message": f"m{i}"} for i in range(6)])

    def newest():
        r = logs.get_log_events(logGroupName=name, logStreamName="app", limit=2)
        assert len(r["events"]) == 2
        return r

    tail = eventually(newest, timeout=60)
    snapshot.match("newest-two", [e["message"] for e in tail["events"]])
    before = logs.get_log_events(logGroupName=name, logStreamName="app", limit=2, nextToken=tail["nextBackwardToken"])
    snapshot.match("two-before", [e["message"] for e in before["events"]])
    head = logs.get_log_events(logGroupName=name, logStreamName="app", limit=3, startFromHead=True)
    snapshot.match("first-three", [e["message"] for e in head["events"]])
    after = logs.get_log_events(logGroupName=name, logStreamName="app", limit=3, nextToken=head["nextForwardToken"])
    snapshot.match("next-three", [e["message"] for e in after["events"]])
    window = logs.get_log_events(logGroupName=name, logStreamName="app", startFromHead=True,
                                 startTime=now + 1, endTime=now + 3)
    snapshot.match("window", [e["message"] for e in window["events"]])


def test_metric_filters_subscription_filters_and_tags_by_arn(client, names, cleanup, snapshot):
    logs = client("logs")
    name = group(logs, names, cleanup)
    snapshot.match("put-metric-filter", logs.put_metric_filter(
        logGroupName=name, filterName="errors", filterPattern="ERROR",
        metricTransformations=[{"metricName": "Errors", "metricNamespace": f"dzc/{names('ns')}", "metricValue": "1"}]))
    snapshot.match("describe-metric-filters", logs.describe_metric_filters(logGroupName=name), opaque=("creationTime",))
    snapshot.match("test-metric-filter", logs.test_metric_filter(
        filterPattern="[ip, status = 4*]", logEventMessages=["1.2.3.4 404", "1.2.3.4 200"]))
    snapshot.match("delete-metric-filter", logs.delete_metric_filter(logGroupName=name, filterName="errors"))
    arn = logs.describe_log_groups(logGroupNamePrefix=name)["logGroups"][0]["logGroupArn"]
    snapshot.match("tag-by-arn", logs.tag_resource(resourceArn=arn, tags={"team": "a"}))
    snapshot.match("tags-by-arn", logs.list_tags_for_resource(resourceArn=arn))
    snapshot.match("untag-by-arn", logs.untag_resource(resourceArn=arn, tagKeys=["team"]))
