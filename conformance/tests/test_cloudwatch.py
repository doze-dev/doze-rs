"""CloudWatch metrics and alarms through boto3.

Cost on AWS: a custom metric and an alarm are each billed by the month,
prorated by the hour, and each one here lives for a couple of minutes.

A datapoint can take a minute or two to become readable on AWS, so the reads
poll; against doze-aws they return at once.
"""

import datetime

# When an alarm was last touched, and its ARN's dependence on nothing but names.
CLOCK = ("AlarmConfigurationUpdatedTimestamp", "StateUpdatedTimestamp", "StateTransitionedTimestamp",
         "Timestamp")


def now():
    return datetime.datetime.now(datetime.timezone.utc)


def test_metric_data_reads_back_as_statistics(client, names, cleanup, snapshot, eventually):
    cw = client("cloudwatch")
    ns = f"dzc/{names('ns')}"
    dims = [{"Name": "queue", "Value": "orders"}]
    at = now().replace(second=0, microsecond=0) - datetime.timedelta(minutes=2)
    snapshot.match("put", cw.put_metric_data(Namespace=ns, MetricData=[
        {"MetricName": "depth", "Dimensions": dims, "Timestamp": at, "Value": v, "Unit": "Count"}
        for v in (1.0, 2.0, 6.0)]))
    cw.put_metric_data(Namespace=ns, MetricData=[{
        "MetricName": "latency", "Timestamp": at, "Unit": "Milliseconds",
        "StatisticValues": {"SampleCount": 4, "Sum": 100, "Minimum": 10, "Maximum": 40}}])

    window = dict(StartTime=at - datetime.timedelta(minutes=5),
                  EndTime=at + datetime.timedelta(minutes=5), Period=60)

    def stats(metric, dimensions=()):
        def read():
            r = cw.get_metric_statistics(
                Namespace=ns, MetricName=metric, Dimensions=list(dimensions),
                Statistics=["SampleCount", "Sum", "Average", "Minimum", "Maximum"], **window)
            assert r["Datapoints"], "no datapoints yet"
            return r
        return eventually(read, timeout=240, every=5)

    snapshot.match("statistics", stats("depth", dims), opaque=CLOCK)
    snapshot.match("statistic-set", stats("latency"), opaque=CLOCK)
    # Dimensions are part of a metric's identity: no dimensions is another metric.
    snapshot.match("without-its-dimensions", cw.get_metric_statistics(
        Namespace=ns, MetricName="depth", Statistics=["Sum"], **window))

    def listed():
        r = cw.list_metrics(Namespace=ns)
        assert len(r["Metrics"]) >= 2, f"{len(r['Metrics'])} of 2 listed so far"
        return r

    snapshot.match("list", eventually(listed, timeout=900, every=10), unordered=("Metrics",))
    snapshot.match("get-metric-data", cw.get_metric_data(
        MetricDataQueries=[{"Id": "d", "MetricStat": {
            "Metric": {"Namespace": ns, "MetricName": "depth", "Dimensions": dims},
            "Period": 60, "Stat": "Sum"}}],
        StartTime=window["StartTime"], EndTime=window["EndTime"]), opaque=("Timestamps",))


def test_alarm_lifecycle(client, names, cleanup, snapshot):
    cw = client("cloudwatch")
    name = names("alarm")
    snapshot.match("put", cw.put_metric_alarm(
        AlarmName=name, AlarmDescription="queue is backing up",
        Namespace=f"dzc/{names('ns')}", MetricName="depth", Statistic="Maximum",
        Dimensions=[{"Name": "queue", "Value": "orders"}],
        Period=60, EvaluationPeriods=2, DatapointsToAlarm=2, Threshold=100,
        ComparisonOperator="GreaterThanThreshold", TreatMissingData="notBreaching",
        ActionsEnabled=False))
    cleanup(cw.delete_alarms, AlarmNames=[name])

    snapshot.match("describe", cw.describe_alarms(AlarmNames=[name]), opaque=CLOCK,
                   drop=("StateValue", "StateReason", "StateReasonData"))
    snapshot.match("set-state", cw.set_alarm_state(
        AlarmName=name, StateValue="ALARM", StateReason="forced by the scenario"))
    snapshot.match("in-alarm", [(a["StateValue"], a["StateReason"]) for a in cw.describe_alarms(
        AlarmNames=[name])["MetricAlarms"]])
    snapshot.match("disable-actions", cw.disable_alarm_actions(AlarmNames=[name]))
    snapshot.match("tag", cw.tag_resource(
        ResourceARN=cw.describe_alarms(AlarmNames=[name])["MetricAlarms"][0]["AlarmArn"],
        Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("by-prefix", [a["AlarmName"] for a in cw.describe_alarms(
        AlarmNamePrefix=name)["MetricAlarms"]])
    snapshot.match("delete", cw.delete_alarms(AlarmNames=[name]))
    snapshot.match("gone", cw.describe_alarms(AlarmNames=[name]))
    snapshot.outcome("delete-again", lambda: cw.delete_alarms(AlarmNames=[name]))


def test_alarm_periods_aws_refuses(client, names, cleanup, snapshot):
    """The model documents an alarm's Period as 10, 20, 30 or a multiple of 60, and Period x EvaluationPeriods at most a day for a
    period under an hour: the others are refused before an alarm is made."""
    cw = client("cloudwatch")
    ns, name = f"dzc/{names('ns')}", names("alarm")
    cleanup(cw.delete_alarms, AlarmNames=[name])
    alarm = dict(AlarmName=name, Namespace=ns, MetricName="m", Statistic="Sum", Period=60,
                 EvaluationPeriods=1, Threshold=1, ComparisonOperator="GreaterThanThreshold")
    snapshot.error("period-five-seconds", lambda: cw.put_metric_alarm(**{**alarm, "Period": 5}))
    snapshot.error("period-forty-five-seconds", lambda: cw.put_metric_alarm(**{**alarm, "Period": 45}))
    snapshot.error("window-over-a-day", lambda: cw.put_metric_alarm(**{**alarm, "EvaluationPeriods": 1441}))
    snapshot.match("period-ten-seconds", cw.put_metric_alarm(**{**alarm, "Period": 10}))


def test_refusals(client, names, cleanup, snapshot):
    cw = client("cloudwatch")
    ns, name = f"dzc/{names('ns')}", names("alarm")
    cleanup(cw.delete_alarms, AlarmNames=[name])
    alarm = dict(AlarmName=name, Namespace=ns, MetricName="m", Statistic="Sum", Period=60,
                 EvaluationPeriods=1, Threshold=1, ComparisonOperator="GreaterThanThreshold")

    # The docs reserve the AWS/ prefix; AWS accepted PutMetricData to AWS/Mine (recorded 2026-11-09), so this is a question.
    snapshot.outcome("reserved-namespace", lambda: cw.put_metric_data(
        Namespace="AWS/Mine", MetricData=[{"MetricName": "m", "Value": 1}]))
    snapshot.error("value-and-statistics", lambda: cw.put_metric_data(Namespace=ns, MetricData=[{
        "MetricName": "m", "Value": 1,
        "StatisticValues": {"SampleCount": 1, "Sum": 1, "Minimum": 1, "Maximum": 1}}]))
    snapshot.error("bad-unit", lambda: cw.put_metric_data(
        Namespace=ns, MetricData=[{"MetricName": "m", "Value": 1, "Unit": "Furlongs"}]))
    snapshot.error("period-not-a-minute", lambda: cw.get_metric_statistics(
        Namespace=ns, MetricName="m", Statistics=["Sum"], Period=45,
        StartTime=now() - datetime.timedelta(hours=1), EndTime=now()))
    snapshot.error("end-before-start", lambda: cw.get_metric_statistics(
        Namespace=ns, MetricName="m", Statistics=["Sum"], Period=60,
        StartTime=now(), EndTime=now() - datetime.timedelta(hours=1)))
    snapshot.error("no-statistic", lambda: cw.get_metric_statistics(
        Namespace=ns, MetricName="m", Period=60,
        StartTime=now() - datetime.timedelta(hours=1), EndTime=now()))

    snapshot.error("bad-comparison", lambda: cw.put_metric_alarm(
        **{**alarm, "ComparisonOperator": "RoughlyEqualTo"}))
    snapshot.error("more-datapoints-than-periods", lambda: cw.put_metric_alarm(
        **alarm, DatapointsToAlarm=3))
    snapshot.error("statistic-and-extended", lambda: cw.put_metric_alarm(
        **alarm, ExtendedStatistic="p99"))
    snapshot.error("bad-treat-missing", lambda: cw.put_metric_alarm(
        **alarm, TreatMissingData="shrug"))
    snapshot.error("set-state-of-absent-alarm", lambda: cw.set_alarm_state(
        AlarmName=names("absent"), StateValue="OK", StateReason="x"))
    snapshot.error("bad-state", lambda: cw.set_alarm_state(
        AlarmName=name, StateValue="PANIC", StateReason="x"))


# Written ahead of a recording (2026-11-12): UNVERIFIED until recorded against AWS.

def test_composite_alarms_dashboards_and_history(client, names, cleanup, snapshot):
    cw = client("cloudwatch")
    base = names("alarm")
    cw.put_metric_alarm(AlarmName=base, Namespace=f"dzc/{names('ns')}", MetricName="m", Statistic="Sum", Period=60,
                        EvaluationPeriods=1, Threshold=1, ComparisonOperator="GreaterThanThreshold")
    comp = names("composite")
    cleanup(cw.delete_alarms, AlarmNames=[comp, base])
    snapshot.match("put-composite", cw.put_composite_alarm(AlarmName=comp, AlarmRule=f"ALARM({base})"))
    d = cw.describe_alarms(AlarmNames=[comp], AlarmTypes=["CompositeAlarm"])
    snapshot.match("describe-composite", d, opaque=CLOCK, drop=("StateValue", "StateReason", "StateReasonData"))
    cw.set_alarm_state(AlarmName=base, StateValue="ALARM", StateReason="forced")
    h = cw.describe_alarm_history(AlarmName=base, HistoryItemType="StateUpdate")
    snapshot.match("history", [i["HistorySummary"] for i in h["AlarmHistoryItems"]])

    dash = names("dash")
    cleanup(cw.delete_dashboards, DashboardNames=[dash])
    body = '{"widgets":[{"type":"text","x":0,"y":0,"width":6,"height":3,"properties":{"markdown":"hello"}}]}'
    snapshot.match("put-dashboard", cw.put_dashboard(DashboardName=dash, DashboardBody=body))
    got = cw.get_dashboard(DashboardName=dash)
    snapshot.match("get-dashboard", {"same-body": got["DashboardBody"] == body})
    snapshot.match("list-dashboards", [e["DashboardName"] for e in cw.list_dashboards(DashboardNamePrefix=dash)["DashboardEntries"]])
    snapshot.match("delete-dashboard", cw.delete_dashboards(DashboardNames=[dash]))
    snapshot.error("get-deleted-dashboard", lambda: cw.get_dashboard(DashboardName=dash))


# Written ahead of a recording (2026-11-19, small-ops): UNVERIFIED until recorded against AWS.

def test_metric_math_in_get_metric_data(client, names, cleanup, snapshot, eventually):
    """Expressions over two metrics: arithmetic, an aggregate over METRICS(), FILL and IF. The values are the question; the
    labels of expressions with no Label are held too."""
    cw = client("cloudwatch")
    ns = f"dzc/{names('ns')}"
    start = now().replace(second=0, microsecond=0) - datetime.timedelta(minutes=4)
    for k, (a, b) in enumerate([(10.0, 4.0), (20.0, 5.0), (40.0, 8.0)]):
        at = start + datetime.timedelta(minutes=k)
        cw.put_metric_data(Namespace=ns, MetricData=[
            {"MetricName": "a", "Timestamp": at, "Value": a, "Unit": "Count"},
            {"MetricName": "b", "Timestamp": at, "Value": b, "Unit": "Count"}])

    def stat(id_, metric):
        return {"Id": id_, "ReturnData": False, "MetricStat": {
            "Metric": {"Namespace": ns, "MetricName": metric}, "Period": 60, "Stat": "Sum"}}

    def read():
        r = cw.get_metric_data(
            MetricDataQueries=[stat("m1", "a"), stat("m2", "b"),
                               {"Id": "ratio", "Expression": "m1 / m2"},
                               {"Id": "total", "Expression": "SUM(METRICS())", "Label": "both"},
                               {"Id": "cond", "Expression": "IF(m1 > 15, m1, -1)"},
                               {"Id": "filled", "Expression": "FILL(m1, 0)"}],
            StartTime=start - datetime.timedelta(minutes=1), EndTime=start + datetime.timedelta(minutes=4),
            ScanBy="TimestampAscending")
        ratio = next(x for x in r["MetricDataResults"] if x["Id"] == "ratio")
        assert len(ratio["Values"]) == 3, "not all datapoints readable yet"
        return r

    snapshot.match("math", eventually(read, timeout=240, every=5), opaque=("Timestamps",))


def test_metric_math_refusals(client, names, snapshot):
    cw = client("cloudwatch")
    window = dict(StartTime=now() - datetime.timedelta(hours=1), EndTime=now())
    snapshot.error("unknown-id", lambda: cw.get_metric_data(
        MetricDataQueries=[{"Id": "e", "Expression": "m9 * 2"}], **window))
    snapshot.error("syntax", lambda: cw.get_metric_data(
        MetricDataQueries=[{"Id": "e", "Expression": "m1 +"}], **window))
    snapshot.error("circular", lambda: cw.get_metric_data(
        MetricDataQueries=[{"Id": "e", "Expression": "f"}, {"Id": "f", "Expression": "e + 1"}], **window))


def test_search_expression(client, names, snapshot, eventually):
    cw = client("cloudwatch")
    ns = f"dzc/{names('ns')}"
    at = now().replace(second=0, microsecond=0) - datetime.timedelta(minutes=2)
    for q, v in (("orders", 1.0), ("billing", 2.0)):
        cw.put_metric_data(Namespace=ns, MetricData=[{
            "MetricName": "depth", "Timestamp": at, "Value": v, "Dimensions": [{"Name": "queue", "Value": q}]}])

    def read():
        r = cw.get_metric_data(
            MetricDataQueries=[{"Id": "s", "Expression": f"SEARCH('{{{ns},queue}} MetricName=\"depth\"', 'Sum', 60)"}],
            StartTime=at - datetime.timedelta(minutes=5), EndTime=at + datetime.timedelta(minutes=5))
        assert len(r["MetricDataResults"]) == 2, "the metrics are not searchable yet"
        return r

    # A search finds metrics only once they are indexed, which can take minutes on AWS.
    snapshot.match("search", eventually(read, timeout=900, every=15), opaque=("Timestamps",),
                   unordered=("MetricDataResults",))


def test_math_alarm(client, names, cleanup, snapshot):
    cw = client("cloudwatch")
    ns, name = f"dzc/{names('ns')}", names("alarm")
    cleanup(cw.delete_alarms, AlarmNames=[name])

    def stat(id_, metric):
        return {"Id": id_, "ReturnData": False, "MetricStat": {
            "Metric": {"Namespace": ns, "MetricName": metric}, "Period": 60, "Stat": "Maximum"}}

    metrics = [stat("m1", "a"), stat("m2", "b"), {"Id": "e", "Expression": "m1 + m2", "ReturnData": True}]
    alarm = dict(AlarmName=name, EvaluationPeriods=1, Threshold=100, ComparisonOperator="GreaterThanThreshold",
                 TreatMissingData="notBreaching", ActionsEnabled=False)
    snapshot.match("put", cw.put_metric_alarm(**alarm, Metrics=metrics))
    snapshot.match("describe", cw.describe_alarms(AlarmNames=[name]), opaque=CLOCK,
                   drop=("StateValue", "StateReason", "StateReasonData"))
    snapshot.error("two-return-data", lambda: cw.put_metric_alarm(
        **alarm, Metrics=[stat("m1", "a"), {"Id": "e", "Expression": "m1"}, {"Id": "f", "Expression": "m1"}]))
    snapshot.error("metrics-and-namespace", lambda: cw.put_metric_alarm(**alarm, Metrics=metrics, Namespace=ns))
    snapshot.match("delete", cw.delete_alarms(AlarmNames=[name]))


def test_log_alarm(client, names, cleanup, snapshot):
    """A log alarm on a Logs Insights query. Cost on AWS: a scheduled query runs every five minutes for as long as the alarm
    lives (a minute or two here). The role must exist on AWS for the put to be accepted (memory); locally any ARN is kept."""
    cw, logs = client("cloudwatch"), client("logs")
    group, name = f"/dzc/{names('group')}", names("alarm")
    logs.create_log_group(logGroupName=group)
    cleanup(logs.delete_log_group, logGroupName=group)
    cleanup(cw.delete_alarms, AlarmNames=[name])
    account = client("sts").get_caller_identity()["Account"]
    alarm = dict(
        AlarmName=name, Threshold=5, ComparisonOperator="GreaterThanThreshold",
        QueryResultsToEvaluate=3, QueryResultsToAlarm=2, ActionsEnabled=False,
        ScheduledQueryConfiguration={
            "QueryString": "fields @message | filter @message like /ERROR/", "LogGroupIdentifiers": [group],
            "ScheduledQueryRoleARN": f"arn:aws:iam::{account}:role/dzc-log-alarm",
            "AggregationExpression": "count(*)",
            "ScheduleConfiguration": {"ScheduleExpression": "rate(5 minutes)", "StartTimeOffset": 300}})
    snapshot.match("put", cw.put_log_alarm(**alarm))
    snapshot.match("describe", cw.describe_alarms(AlarmNames=[name], AlarmTypes=["LogAlarm"]), opaque=CLOCK + ("QueryARN",),
                   drop=("StateValue", "StateReason", "StateReasonData", "EvaluationState"))
    snapshot.match("not-a-metric-alarm", cw.describe_alarms(AlarmNames=[name])["MetricAlarms"])
    snapshot.error("more-to-alarm-than-evaluated", lambda: cw.put_log_alarm(**{**alarm, "QueryResultsToAlarm": 4}))
    snapshot.match("delete", cw.delete_alarms(AlarmNames=[name]))


# Written ahead of a recording (2026-11-19, final-ops): UNVERIFIED until recorded against AWS.

def test_describe_alarms_shows_metric_alarms_unless_asked(client, names, cleanup, snapshot):
    """The model: with no AlarmTypes, "only metric alarms are returned, even if composite alarms or log alarms exist"."""
    cw = client("cloudwatch")
    base, comp = names("alarm"), names("composite")
    cw.put_metric_alarm(AlarmName=base, Namespace=f"dzc/{names('ns')}", MetricName="m", Statistic="Sum", Period=60,
                        EvaluationPeriods=1, Threshold=1, ComparisonOperator="GreaterThanThreshold", ActionsEnabled=False)
    cw.put_composite_alarm(AlarmName=comp, AlarmRule=f"ALARM({base})", ActionsEnabled=False)
    cleanup(cw.delete_alarms, AlarmNames=[comp, base])

    def shown(**kw):
        d = cw.describe_alarms(AlarmNames=[base, comp], **kw)
        return {k: [a["AlarmName"] == base for a in d.get(k, [])] for k in ("MetricAlarms", "CompositeAlarms", "LogAlarms")}
    snapshot.match("no-types", shown())
    snapshot.match("composite-only", shown(AlarmTypes=["CompositeAlarm"]))
    snapshot.match("both", shown(AlarmTypes=["MetricAlarm", "CompositeAlarm"]))


def test_a_composite_alarm_follows_its_rule(client, names, cleanup, snapshot, eventually):
    cw = client("cloudwatch")
    ns, cpu, disk, comp = f"dzc/{names('ns')}", names("cpu"), names("disk"), names("composite")
    for a in (cpu, disk):
        cw.put_metric_alarm(AlarmName=a, Namespace=ns, MetricName=a, Statistic="Sum", Period=60, EvaluationPeriods=1,
                            Threshold=1, ComparisonOperator="GreaterThanThreshold", ActionsEnabled=False)
    cw.put_composite_alarm(AlarmName=comp, AlarmRule=f'ALARM({cpu}) AND ALARM("{disk}")', ActionsEnabled=False)
    cleanup(cw.delete_alarms, AlarmNames=[comp, cpu, disk])

    def state(want):
        def read():
            s = cw.describe_alarms(AlarmNames=[comp], AlarmTypes=["CompositeAlarm"])["CompositeAlarms"][0]["StateValue"]
            assert s == want, s
            return s
        return eventually(read, timeout=120, every=5)

    # Evaluated once it is made: the rule does not hold.
    snapshot.match("made", state("OK"))
    for a in (cpu, disk):
        cw.set_alarm_state(AlarmName=a, StateValue="ALARM", StateReason="forced by the scenario")
    snapshot.match("both-children-in-alarm", state("ALARM"))
    cw.set_alarm_state(AlarmName=cpu, StateValue="OK", StateReason="forced by the scenario")
    snapshot.match("one-child-ok", state("OK"))
    snapshot.error("rule-that-does-not-read", lambda: cw.put_composite_alarm(AlarmName=comp, AlarmRule=f"ALARM({cpu}"))


def test_a_log_alarm_is_evaluated_on_its_schedule(client, names, cleanup, snapshot, eventually):
    """A log alarm on a query every minute: three ERROR lines against a threshold of 2. Takes a minute or two on either side. On AWS
    the role `dzc-log-alarm` must exist (not made here)."""
    cw, logs = client("cloudwatch"), client("logs")
    group, name = f"/dzc/{names('group')}", names("alarm")
    logs.create_log_group(logGroupName=group)
    cleanup(logs.delete_log_group, logGroupName=group)
    logs.create_log_stream(logGroupName=group, logStreamName="app")
    cleanup(cw.delete_alarms, AlarmNames=[name])
    account = client("sts").get_caller_identity()["Account"]
    cw.put_log_alarm(
        AlarmName=name, Threshold=2, ComparisonOperator="GreaterThanThreshold",
        QueryResultsToEvaluate=1, QueryResultsToAlarm=1, ActionsEnabled=False,
        ScheduledQueryConfiguration={
            "QueryString": "fields @message | filter @message like /ERROR/", "LogGroupIdentifiers": [group],
            "ScheduledQueryRoleARN": f"arn:aws:iam::{account}:role/dzc-log-alarm",
            "AggregationExpression": "count(*)",
            "ScheduleConfiguration": {"ScheduleExpression": "rate(1 minute)", "StartTimeOffset": 600}})
    at = int(datetime.datetime.now(datetime.timezone.utc).timestamp() * 1000)
    logs.put_log_events(logGroupName=group, logStreamName="app", logEvents=[
        {"timestamp": at, "message": m} for m in ("ERROR one", "INFO fine", "ERROR two", "ERROR three")])

    def alarmed():
        a = cw.describe_alarms(AlarmNames=[name], AlarmTypes=["LogAlarm"])["LogAlarms"][0]
        assert a["StateValue"] == "ALARM", a["StateValue"]
        return a["StateValue"]
    snapshot.match("alarmed", eventually(alarmed, timeout=420, every=10))
