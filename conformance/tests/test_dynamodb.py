"""DynamoDB through boto3's low-level client."""

FAST = {"Delay": 1, "MaxAttempts": 120}

# How much a table holds is updated every six hours on AWS.
WEATHER = ("ItemCount", "TableSizeBytes")


def table(ddb, names, cleanup, sort_key=False):
    name = names("table")
    schema = [{"AttributeName": "pk", "KeyType": "HASH"}]
    attrs = [{"AttributeName": "pk", "AttributeType": "S"}]
    if sort_key:
        schema.append({"AttributeName": "sk", "KeyType": "RANGE"})
        attrs.append({"AttributeName": "sk", "AttributeType": "N"})
    created = ddb.create_table(TableName=name, KeySchema=schema,
                               AttributeDefinitions=attrs, BillingMode="PAY_PER_REQUEST")
    cleanup(ddb.delete_table, TableName=name)
    ddb.get_waiter("table_exists").wait(TableName=name, WaiterConfig=FAST)
    return name, created


def test_table_lifecycle(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, created = table(ddb, names, cleanup)
    # TableStatus is CREATING on AWS at this instant and may be ACTIVE here;
    # how long a table takes is not the contract, what it settles into is.
    snapshot.match("create", created, drop=WEATHER + ("TableStatus",))
    snapshot.match("describe", ddb.describe_table(TableName=name), drop=WEATHER)
    snapshot.match("ttl", ddb.describe_time_to_live(TableName=name))
    snapshot.match("delete", ddb.delete_table(TableName=name), drop=WEATHER + ("TableStatus",))
    ddb.get_waiter("table_not_exists").wait(TableName=name, WaiterConfig=FAST)
    snapshot.error("describe-deleted", lambda: ddb.describe_table(TableName=name))


def test_item_round_trip_and_every_type(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    item = {
        "pk": {"S": "a"},
        "n": {"N": "1.50"},
        "b": {"B": b"\x00\xff"},
        "flag": {"BOOL": True},
        "nothing": {"NULL": True},
        "list": {"L": [{"S": "x"}, {"N": "2"}]},
        "map": {"M": {"inner": {"S": "y"}}},
        "ss": {"SS": ["b", "a"]},
        "ns": {"NS": ["2", "1"]},
    }
    snapshot.match("put", ddb.put_item(TableName=name, Item=item))
    snapshot.match("get", ddb.get_item(TableName=name, Key={"pk": {"S": "a"}}, ConsistentRead=True),
                   unordered=("SS", "NS"))
    snapshot.match("get-absent", ddb.get_item(TableName=name, Key={"pk": {"S": "zz"}}))
    snapshot.match("put-returning-old", ddb.put_item(
        TableName=name, Item={"pk": {"S": "a"}, "n": {"N": "2"}}, ReturnValues="ALL_OLD"),
        unordered=("SS", "NS"))

    snapshot.match("update", ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}},
        UpdateExpression="SET #n = #n + :one, tags = :t ADD seen :s REMOVE gone",
        ExpressionAttributeNames={"#n": "n"},
        ExpressionAttributeValues={":one": {"N": "1"}, ":t": {"L": []}, ":s": {"SS": ["x"]}},
        ReturnValues="ALL_NEW"))
    snapshot.match("update-updated-old", ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}},
        UpdateExpression="SET n = :v", ExpressionAttributeValues={":v": {"N": "9"}},
        ReturnValues="UPDATED_OLD"))
    snapshot.match("delete", ddb.delete_item(
        TableName=name, Key={"pk": {"S": "a"}}, ReturnValues="ALL_OLD"))


def test_query_and_scan(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup, sort_key=True)
    for sk in range(1, 6):
        ddb.put_item(TableName=name, Item={
            "pk": {"S": "a"}, "sk": {"N": str(sk)}, "even": {"BOOL": sk % 2 == 0}})
    ddb.put_item(TableName=name, Item={"pk": {"S": "b"}, "sk": {"N": "1"}})

    q = dict(TableName=name, KeyConditionExpression="pk = :p",
             ExpressionAttributeValues={":p": {"S": "a"}}, ConsistentRead=True)
    snapshot.match("all", ddb.query(**q))
    snapshot.match("descending", ddb.query(**q, ScanIndexForward=False, Limit=2))
    page = snapshot.match("page-1", ddb.query(**q, Limit=2))
    snapshot.match("page-2", ddb.query(**q, Limit=2, ExclusiveStartKey=page["LastEvaluatedKey"]))
    snapshot.match("between", ddb.query(
        TableName=name, KeyConditionExpression="pk = :p AND sk BETWEEN :lo AND :hi",
        ExpressionAttributeValues={":p": {"S": "a"}, ":lo": {"N": "2"}, ":hi": {"N": "4"}},
        ConsistentRead=True))
    # A filter is applied after the limit, so Count and ScannedCount part ways.
    snapshot.match("filtered", ddb.query(
        TableName=name, KeyConditionExpression="pk = :p", FilterExpression="even = :t",
        ExpressionAttributeValues={":p": {"S": "a"}, ":t": {"BOOL": True}},
        Limit=3, ConsistentRead=True))
    snapshot.match("projection", ddb.query(**q, ProjectionExpression="sk", Limit=1))
    snapshot.match("count", ddb.query(**q, Select="COUNT"))
    snapshot.match("scan", ddb.scan(TableName=name, ConsistentRead=True), unordered=("Items",))


def test_conditions_and_transactions(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "n": {"N": "1"}})

    snapshot.error("condition-fails", lambda: ddb.put_item(
        TableName=name, Item={"pk": {"S": "a"}},
        ConditionExpression="attribute_not_exists(pk)"))
    snapshot.error("condition-fails-returning-old", lambda: ddb.put_item(
        TableName=name, Item={"pk": {"S": "a"}},
        ConditionExpression="attribute_not_exists(pk)",
        ReturnValuesOnConditionCheckFailure="ALL_OLD"))

    snapshot.match("transact", ddb.transact_write_items(TransactItems=[
        {"Put": {"TableName": name, "Item": {"pk": {"S": "b"}}}},
        {"Update": {"TableName": name, "Key": {"pk": {"S": "a"}},
                    "UpdateExpression": "SET n = :v",
                    "ExpressionAttributeValues": {":v": {"N": "2"}}}},
    ]))
    snapshot.error("transact-cancelled", lambda: ddb.transact_write_items(TransactItems=[
        {"Put": {"TableName": name, "Item": {"pk": {"S": "c"}}}},
        {"ConditionCheck": {"TableName": name, "Key": {"pk": {"S": "a"}},
                            "ConditionExpression": "n = :v",
                            "ExpressionAttributeValues": {":v": {"N": "999"}}}},
    ]))
    snapshot.match("nothing-was-written",
                   ddb.get_item(TableName=name, Key={"pk": {"S": "c"}}, ConsistentRead=True))
    snapshot.match("batch-get", ddb.batch_get_item(RequestItems={name: {
        "Keys": [{"pk": {"S": "a"}}, {"pk": {"S": "b"}}, {"pk": {"S": "zz"}}],
        "ConsistentRead": True}}), unordered=(names("table"),))


def test_refusals(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    snapshot.error("no-such-table", lambda: ddb.get_item(
        TableName=names("absent"), Key={"pk": {"S": "a"}}))

    name, _ = table(ddb, names, cleanup)
    snapshot.error("table-exists", lambda: ddb.create_table(
        TableName=name, KeySchema=[{"AttributeName": "pk", "KeyType": "HASH"}],
        AttributeDefinitions=[{"AttributeName": "pk", "AttributeType": "S"}],
        BillingMode="PAY_PER_REQUEST"))
    snapshot.error("missing-key", lambda: ddb.put_item(TableName=name, Item={"other": {"S": "x"}}))
    snapshot.error("wrong-key-type", lambda: ddb.put_item(TableName=name, Item={"pk": {"N": "1"}}))
    snapshot.error("bad-expression", lambda: ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET"))
    snapshot.error("undefined-value", lambda: ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET n = :missing"))
    snapshot.error("unused-value", lambda: ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET n = :v",
        ExpressionAttributeValues={":v": {"N": "1"}, ":unused": {"N": "2"}}))
    snapshot.error("reserved-word", lambda: ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET name = :v",
        ExpressionAttributeValues={":v": {"S": "x"}}))
    snapshot.error("update-the-key", lambda: ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET pk = :v",
        ExpressionAttributeValues={":v": {"S": "b"}}))
    snapshot.error("empty-set", lambda: ddb.put_item(
        TableName=name, Item={"pk": {"S": "a"}, "s": {"SS": []}}))


def test_global_secondary_index(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    name = names("table")
    created = ddb.create_table(
        TableName=name, BillingMode="PAY_PER_REQUEST",
        KeySchema=[{"AttributeName": "pk", "KeyType": "HASH"}],
        AttributeDefinitions=[{"AttributeName": "pk", "AttributeType": "S"},
                              {"AttributeName": "owner", "AttributeType": "S"},
                              {"AttributeName": "at", "AttributeType": "N"}],
        GlobalSecondaryIndexes=[{
            "IndexName": "by-owner",
            "KeySchema": [{"AttributeName": "owner", "KeyType": "HASH"},
                          {"AttributeName": "at", "KeyType": "RANGE"}],
            "Projection": {"ProjectionType": "INCLUDE", "NonKeyAttributes": ["title"]},
        }])
    cleanup(ddb.delete_table, TableName=name)
    ddb.get_waiter("table_exists").wait(TableName=name, WaiterConfig=FAST)
    status = ("TableStatus", "IndexStatus")
    snapshot.match("create", created, drop=WEATHER + status)
    snapshot.match("describe", ddb.describe_table(TableName=name), drop=WEATHER + status)

    for i, owner in enumerate(["ada", "ada", "bob"]):
        ddb.put_item(TableName=name, Item={
            "pk": {"S": f"doc-{i}"}, "owner": {"S": owner}, "at": {"N": str(i)},
            "title": {"S": f"t{i}"}, "body": {"S": "not projected"}})
    ddb.put_item(TableName=name, Item={"pk": {"S": "no-owner"}})  # sparse: absent from the index

    q = dict(TableName=name, IndexName="by-owner", KeyConditionExpression="#o = :o",
             ExpressionAttributeNames={"#o": "owner"}, ExpressionAttributeValues={":o": {"S": "ada"}})

    def indexed():
        r = ddb.query(**q)
        assert r["Count"] == 2, f"{r['Count']} of 2 in the index so far"
        return r

    snapshot.match("query-index", eventually(indexed))
    snapshot.match("newest-first", ddb.query(**q, ScanIndexForward=False, Limit=1))
    snapshot.match("scan-index-is-sparse", ddb.scan(TableName=name, IndexName="by-owner", Select="COUNT"))
    snapshot.error("consistent-read-on-a-gsi", lambda: ddb.query(**q, ConsistentRead=True))
    snapshot.error("absent-index", lambda: ddb.query(**{**q, "IndexName": "absent"}))
    snapshot.error("all-attributes-not-projected", lambda: ddb.query(**q, Select="ALL_ATTRIBUTES"))
    # "owner" unaliased: a reserved word, and the reason for the #o above.
    snapshot.error("reserved-word-in-key-condition", lambda: ddb.query(
        TableName=name, IndexName="by-owner", KeyConditionExpression="owner = :o",
        ExpressionAttributeValues={":o": {"S": "ada"}}))


def test_batch_write_and_ttl(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    put = lambda k: {"PutRequest": {"Item": {"pk": {"S": k}}}}
    snapshot.match("write", ddb.batch_write_item(RequestItems={name: [put("a"), put("b"), put("c")]}))
    snapshot.match("delete-one", ddb.batch_write_item(RequestItems={name: [
        {"DeleteRequest": {"Key": {"pk": {"S": "a"}}}}, put("d")]}))
    snapshot.match("count", ddb.scan(TableName=name, Select="COUNT", ConsistentRead=True))
    snapshot.error("same-key-twice", lambda: ddb.batch_write_item(
        RequestItems={name: [put("z"), put("z")]}))
    snapshot.error("twenty-six", lambda: ddb.batch_write_item(
        RequestItems={name: [put(f"k{i}") for i in range(26)]}))

    spec = {"Enabled": True, "AttributeName": "expires"}
    snapshot.match("enable-ttl", ddb.update_time_to_live(TableName=name, TimeToLiveSpecification=spec))
    snapshot.error("enable-ttl-again", lambda: ddb.update_time_to_live(
        TableName=name, TimeToLiveSpecification=spec))


# Scenarios below were written before AWS answered them (2026-11-12): each runs against doze-aws and is UNVERIFIED until recorded.
# Read the diff of every recording before keeping it; where AWS contradicts a scenario, it becomes snapshot.outcome with a dated note.


def test_point_in_time_recovery_is_a_round_trip(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    snapshot.match("before", ddb.describe_continuous_backups(TableName=name))
    on = ddb.update_continuous_backups(
        TableName=name, PointInTimeRecoverySpecification={"PointInTimeRecoveryEnabled": True, "RecoveryPeriodInDays": 7})
    snapshot.match("enable", on)
    snapshot.match("after", ddb.describe_continuous_backups(TableName=name))
    snapshot.error("absent-table", lambda: ddb.describe_continuous_backups(TableName=names("absent")))


def test_tags(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    name, created = table(ddb, names, cleanup)
    # Deleting is paced like tagging: after the 2026-11-15 recording the table stayed behind, its plain delete most likely refused
    # while the tags settled (the cleanup does not say why).
    cleanup(paced, lambda: ddb.delete_table(TableName=name))
    arn = created["TableDescription"]["TableArn"]
    snapshot.match("tag", ddb.tag_resource(ResourceArn=arn, Tags=[{"Key": "team", "Value": "a"}, {"Key": "env", "Value": "dev"}]))

    def tagged():
        r = ddb.list_tags_of_resource(ResourceArn=arn)
        assert len(r["Tags"]) == 2, "tags are eventually consistent"
        return r

    snapshot.match("list", eventually(tagged), unordered=("Tags",))
    # AWS paces tag changes on a table: one made while the last is still being applied is refused with LimitExceededException
    # ("Subscriber limit exceeded: Table tags are being updated", recorded 2026-11-15). Wait it out rather than record it.
    snapshot.match("untag", paced(lambda: ddb.untag_resource(ResourceArn=arn, TagKeys=["env"])))
    snapshot.error("reserved-prefix", lambda: paced(
        lambda: ddb.tag_resource(ResourceArn=arn, Tags=[{"Key": "aws:x", "Value": "a"}])))


def paced(fn, timeout=120):
    """Call fn again while AWS says the table's tags are still being updated."""
    import time

    from botocore.exceptions import ClientError

    deadline = time.time() + timeout
    while True:
        try:
            return fn()
        except ClientError as e:
            if e.response["Error"]["Code"] != "LimitExceededException" or time.time() > deadline:
                raise
            time.sleep(2)


def test_stream_specification_on_an_existing_table(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    spec = {"StreamEnabled": True, "StreamViewType": "NEW_AND_OLD_IMAGES"}
    on = ddb.update_table(TableName=name, StreamSpecification=spec)
    snapshot.match("enable", on, drop=WEATHER + ("TableStatus", "LatestStreamArn", "LatestStreamLabel"))
    ddb.get_waiter("table_exists").wait(TableName=name, WaiterConfig=FAST)
    snapshot.error("enable-again", lambda: ddb.update_table(TableName=name, StreamSpecification=spec))
    snapshot.error("no-change-at-all", lambda: ddb.update_table(TableName=name))


def test_update_expression_forms(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    ddb.put_item(TableName=name, Item={
        "pk": {"S": "a"}, "l": {"L": [{"N": "1"}]}, "l2": {"L": [{"N": "1"}, {"N": "2"}, {"N": "3"}]}, "m": {"M": {"k": {"S": "v"}}},
        "s": {"SS": ["x", "y", "z"]}, "n": {"N": "10"}})
    key = {"pk": {"S": "a"}}
    snapshot.match("list-append-and-nested", ddb.update_item(
        TableName=name, Key=key, ReturnValues="ALL_NEW",
        UpdateExpression="SET l = list_append(l, :more), m.k2 = :v, c = if_not_exists(c, :zero) REMOVE l2[0]",
        ExpressionAttributeValues={":more": {"L": [{"N": "4"}]}, ":v": {"S": "w"}, ":zero": {"N": "0"}}))
    snapshot.match("set-delete", ddb.update_item(
        TableName=name, Key=key, ReturnValues="UPDATED_NEW",
        UpdateExpression="DELETE s :d ADD n :five",
        ExpressionAttributeValues={":d": {"SS": ["y"]}, ":five": {"N": "5"}}))
    snapshot.error("overlapping-paths", lambda: ddb.update_item(
        TableName=name, Key=key, UpdateExpression="SET m.k = :v REMOVE m",
        ExpressionAttributeValues={":v": {"S": "x"}}))
    snapshot.error("add-to-a-string", lambda: ddb.update_item(
        TableName=name, Key=key, UpdateExpression="ADD m.k :one",
        ExpressionAttributeValues={":one": {"N": "1"}}))
    snapshot.error("arithmetic-on-a-string", lambda: ddb.update_item(
        TableName=name, Key=key, UpdateExpression="SET n = m.k + :one",
        ExpressionAttributeValues={":one": {"N": "1"}}))


def test_scan_pages_and_segments(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    for i in range(7):
        ddb.put_item(TableName=name, Item={"pk": {"S": f"k{i}"}, "n": {"N": str(i)}})
    seen, start = [], None
    while True:
        kw = {"ExclusiveStartKey": start} if start else {}
        page = ddb.scan(TableName=name, Limit=3, ConsistentRead=True, **kw)
        seen += [i["pk"]["S"] for i in page["Items"]]
        start = page.get("LastEvaluatedKey")
        if not start:
            break
    snapshot.match("all-keys", {"keys": sorted(seen)})
    snapshot.match("filtered", ddb.scan(TableName=name, ConsistentRead=True, FilterExpression="n > :t",
                                        ExpressionAttributeValues={":t": {"N": "4"}}), unordered=("Items",))
    counts = [ddb.scan(TableName=name, Segment=s, TotalSegments=3, Select="COUNT", ConsistentRead=True)["Count"] for s in range(3)]
    snapshot.match("segments-add-up", {"total": sum(counts)})
    snapshot.error("segment-out-of-range", lambda: ddb.scan(TableName=name, Segment=3, TotalSegments=3))


def test_transaction_rules(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    same = [{"Put": {"TableName": name, "Item": {"pk": {"S": "a"}}}},
            {"Delete": {"TableName": name, "Key": {"pk": {"S": "a"}}}}]
    snapshot.error("two-operations-on-one-item", lambda: ddb.transact_write_items(TransactItems=same))
    update = lambda v: [{"Update": {"TableName": name, "Key": {"pk": {"S": "ctr"}}, "UpdateExpression": "ADD n :v",
                                    "ExpressionAttributeValues": {":v": {"N": str(v)}}}}]
    token = "dzc-" + names("token")[-12:]
    snapshot.match("first", ddb.transact_write_items(TransactItems=update(1), ClientRequestToken=token))
    snapshot.match("replay", ddb.transact_write_items(TransactItems=update(1), ClientRequestToken=token))
    snapshot.match("applied-once", ddb.get_item(TableName=name, Key={"pk": {"S": "ctr"}}, ConsistentRead=True))
    snapshot.error("same-token-other-request", lambda: ddb.transact_write_items(TransactItems=update(2), ClientRequestToken=token))
    snapshot.error("cancelled-with-the-item", lambda: ddb.transact_write_items(TransactItems=[
        {"Delete": {"TableName": name, "Key": {"pk": {"S": "ctr"}}, "ConditionExpression": "n = :v",
                    "ExpressionAttributeValues": {":v": {"N": "99"}}, "ReturnValuesOnConditionCheckFailure": "ALL_OLD"}}]))
    snapshot.match("get", ddb.transact_get_items(TransactItems=[
        {"Get": {"TableName": name, "Key": {"pk": {"S": "zz"}}}}, {"Get": {"TableName": name, "Key": {"pk": {"S": "ctr"}}}}]))


def test_key_and_item_limits(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    snapshot.error("empty-key", lambda: ddb.put_item(TableName=name, Item={"pk": {"S": ""}}))
    snapshot.error("key-over-2048-bytes", lambda: ddb.put_item(TableName=name, Item={"pk": {"S": "x" * 2049}}))
    snapshot.error("item-over-400kb", lambda: ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "blob": {"S": "x" * (401 * 1024)}}))
    snapshot.error("key-with-extra-attribute", lambda: ddb.get_item(TableName=name, Key={"pk": {"S": "a"}, "x": {"S": "b"}}))
    snapshot.error("duplicate-set-members", lambda: ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "s": {"SS": ["a", "a"]}}))
    snapshot.error("bad-number", lambda: ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "n": {"N": "12abc"}}))
    snapshot.match("number-normalised", ddb.update_item(
        TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET n = :v", ReturnValues="ALL_NEW",
        ExpressionAttributeValues={":v": {"N": "0001.2300"}}))


def test_index_added_and_removed_later(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    for k, c in (("a", "red"), ("b", "blue"), ("c", None)):
        ddb.put_item(TableName=name, Item={"pk": {"S": k}, **({"color": {"S": c}} if c else {})})
    ddb.update_table(
        TableName=name, AttributeDefinitions=[{"AttributeName": "color", "AttributeType": "S"}],
        GlobalSecondaryIndexUpdates=[{"Create": {"IndexName": "by-color", "KeySchema": [{"AttributeName": "color", "KeyType": "HASH"}],
                                                 "Projection": {"ProjectionType": "ALL"}}}])
    ddb.get_waiter("table_exists").wait(TableName=name, WaiterConfig=FAST)

    def ready():
        d = ddb.describe_table(TableName=name)["Table"]["GlobalSecondaryIndexes"][0]
        assert d["IndexStatus"] == "ACTIVE", "the index is still being built"

    # On AWS a new index can take most of an hour to build, even over three items (2026-11-15: more than 15 minutes, done within 50).
    eventually(ready, timeout=3600, every=15)
    q = dict(TableName=name, IndexName="by-color", KeyConditionExpression="color = :c")
    snapshot.match("red", ddb.query(**q, ExpressionAttributeValues={":c": {"S": "red"}}))
    snapshot.error("two-index-updates", lambda: ddb.update_table(
        TableName=name, GlobalSecondaryIndexUpdates=[{"Delete": {"IndexName": "by-color"}}, {"Delete": {"IndexName": "by-color"}}]))


# Written 2026-11-13 with PartiQL, Contributor Insights and the Streams API, before AWS answered them: UNVERIFIED until recorded.


def test_partiql_statements(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup, sort_key=True)
    q = f'"{name}"'
    snapshot.match("insert", ddb.execute_statement(
        Statement=f"INSERT INTO {q} VALUE {{'pk': ?, 'sk': 1, 'title': 'Hi', 'n': 5}}", Parameters=[{"S": "a"}]))
    ddb.execute_statement(Statement=f"INSERT INTO {q} VALUE {{'pk': 'a', 'sk': 2, 'title': 'Ho', 'n': 9}}")
    snapshot.error("insert-duplicate", lambda: ddb.execute_statement(
        Statement=f"INSERT INTO {q} VALUE {{'pk': 'a', 'sk': 1}}"))
    snapshot.match("select-by-key", ddb.execute_statement(
        Statement=f"SELECT * FROM {q} WHERE pk = ? AND sk = ?", Parameters=[{"S": "a"}, {"N": "1"}], ConsistentRead=True))
    snapshot.match("select-filtered", ddb.execute_statement(
        Statement=f"SELECT title FROM {q} WHERE pk = 'a' AND n > ?", Parameters=[{"N": "6"}], ConsistentRead=True))
    snapshot.match("update", ddb.execute_statement(
        Statement=f"UPDATE {q} SET n = n + ? REMOVE title WHERE pk = 'a' AND sk = 1 RETURNING ALL NEW *",
        Parameters=[{"N": "1"}]))
    snapshot.error("update-without-whole-key", lambda: ddb.execute_statement(
        Statement=f"UPDATE {q} SET n = 1 WHERE pk = 'a'"))
    snapshot.error("update-of-nothing", lambda: ddb.execute_statement(
        Statement=f"UPDATE {q} SET n = 1 WHERE pk = 'zz' AND sk = 1"))
    snapshot.match("delete", ddb.execute_statement(
        Statement=f"DELETE FROM {q} WHERE pk = 'a' AND sk = 2 RETURNING ALL OLD *"))
    snapshot.error("parameters-do-not-match", lambda: ddb.execute_statement(
        Statement=f"SELECT * FROM {q} WHERE pk = ?"))
    snapshot.error("not-a-statement", lambda: ddb.execute_statement(Statement=f"SELEKT * FROM {q}"))


def test_partiql_batch_and_transaction(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    q = f'"{name}"'
    ddb.put_item(TableName=name, Item={"pk": {"S": "taken"}})
    snapshot.match("batch-write", ddb.batch_execute_statement(Statements=[
        {"Statement": f"INSERT INTO {q} VALUE {{'pk': 'a'}}"},
        {"Statement": f"INSERT INTO {q} VALUE {{'pk': 'taken'}}"},
        {"Statement": f"UPDATE {q} SET x = 1 WHERE pk = 'nobody'"}]))
    snapshot.match("batch-read", ddb.batch_execute_statement(Statements=[
        {"Statement": f"SELECT * FROM {q} WHERE pk = ?", "Parameters": [{"S": "a"}], "ConsistentRead": True},
        {"Statement": f"SELECT * FROM {q} WHERE pk = 'zz'", "ConsistentRead": True}]))
    snapshot.error("transaction-cancelled", lambda: ddb.execute_transaction(TransactStatements=[
        {"Statement": f"INSERT INTO {q} VALUE {{'pk': 'new'}}"},
        {"Statement": f"INSERT INTO {q} VALUE {{'pk': 'taken'}}"}]))
    snapshot.match("nothing-written", ddb.get_item(TableName=name, Key={"pk": {"S": "new"}}, ConsistentRead=True))
    snapshot.match("transaction", ddb.execute_transaction(TransactStatements=[
        {"Statement": f"INSERT INTO {q} VALUE {{'pk': 'new'}}"},
        {"Statement": f"UPDATE {q} SET n = 1 WHERE pk = 'taken'"}]))
    snapshot.match("read-transaction", ddb.execute_transaction(TransactStatements=[
        {"Statement": f"SELECT * FROM {q} WHERE pk = 'new'"},
        {"Statement": f"SELECT n FROM {q} WHERE pk = 'taken'"}]))


def test_contributor_insights_settings(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    snapshot.match("before", ddb.describe_contributor_insights(TableName=name))
    snapshot.match("enable", ddb.update_contributor_insights(TableName=name, ContributorInsightsAction="ENABLE"))

    def enabled():
        r = ddb.describe_contributor_insights(TableName=name)
        assert r["ContributorInsightsStatus"] == "ENABLED", "still enabling"
        return r

    # AWS takes minutes to turn Contributor Insights on (it makes a service-linked role the first time).
    snapshot.match("enabled", eventually(enabled, timeout=300, every=5), drop=("LastUpdateDateTime", "ContributorInsightsRuleList"))
    snapshot.match("listed", ddb.list_contributor_insights(TableName=name))
    snapshot.match("disable", ddb.update_contributor_insights(TableName=name, ContributorInsightsAction="DISABLE"))
    snapshot.error("absent-table", lambda: ddb.describe_contributor_insights(TableName=names("absent")))


def test_streams_api(client, names, cleanup, snapshot, eventually):
    ddb, streams = client("dynamodb"), client("dynamodbstreams")
    name = names("table")
    ddb.create_table(TableName=name, BillingMode="PAY_PER_REQUEST",
                     KeySchema=[{"AttributeName": "pk", "KeyType": "HASH"}],
                     AttributeDefinitions=[{"AttributeName": "pk", "AttributeType": "S"}],
                     StreamSpecification={"StreamEnabled": True, "StreamViewType": "NEW_AND_OLD_IMAGES"})
    cleanup(ddb.delete_table, TableName=name)
    ddb.get_waiter("table_exists").wait(TableName=name, WaiterConfig=FAST)
    volatile = ("StreamArn", "StreamLabel", "ShardId", "StartingSequenceNumber", "SequenceNumber",
                "CreationRequestDateTime", "ApproximateCreationDateTime", "eventID", "NextShardIterator", "ShardIterator")
    snapshot.match("list", streams.list_streams(TableName=name), drop=volatile)
    arn = streams.list_streams(TableName=name)["Streams"][0]["StreamArn"]
    described = streams.describe_stream(StreamArn=arn)
    snapshot.match("describe", described, drop=volatile)
    # AWS gives a new on-demand table's stream several shards (four, recorded 2026-11-15), and a key's changes are in one of them:
    # read them all.
    shards = [s["ShardId"] for s in described["StreamDescription"]["Shards"]]
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "n": {"N": "1"}})
    ddb.update_item(TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET n = :v",
                    ExpressionAttributeValues={":v": {"N": "2"}})
    ddb.delete_item(TableName=name, Key={"pk": {"S": "a"}})
    its = {s: streams.get_shard_iterator(StreamArn=arn, ShardId=s, ShardIteratorType="TRIM_HORIZON")["ShardIterator"]
           for s in shards}
    got = []

    def all_three():
        for s, it in list(its.items()):
            if it is None:
                continue
            r = streams.get_records(ShardIterator=it)
            got.extend(r["Records"])
            its[s] = r.get("NextShardIterator")
        assert len(got) >= 3, "the records are not all there yet"

    eventually(all_three)
    snapshot.match("records", {"Records": got}, drop=volatile)
    snapshot.error("unknown-shard", lambda: streams.get_shard_iterator(
        StreamArn=arn, ShardId="shardId-00000000000000000000-00000000", ShardIteratorType="LATEST"))
    snapshot.error("bad-iterator", lambda: streams.get_records(ShardIterator="x" * 40))


# --- Legacy parameters (stage dynamodb-ops). Older SDKs and libraries still send these; AWS runs them and refuses a mix.

def legacy_table(ddb, names, cleanup):
    name, _ = table(ddb, names, cleanup, sort_key=True)
    for sk, n, tag in [(1, 10, "red"), (2, 20, "blue"), (3, 30, "red")]:
        ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "sk": {"N": str(sk)}, "n": {"N": str(n)}, "tag": {"S": tag}})
    return name


def test_legacy_reads(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name = legacy_table(ddb, names, cleanup)
    key = {"pk": {"S": "a"}, "sk": {"N": "1"}}
    snapshot.match("get-attributes-to-get", ddb.get_item(TableName=name, Key=key, AttributesToGet=["n"], ConsistentRead=True))
    snapshot.match("query-key-conditions", ddb.query(
        TableName=name, ConsistentRead=True,
        KeyConditions={"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]},
                       "sk": {"ComparisonOperator": "BETWEEN", "AttributeValueList": [{"N": "1"}, {"N": "3"}]}},
        QueryFilter={"tag": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "red"}]}},
        AttributesToGet=["sk", "n"]))
    snapshot.match("query-or", ddb.query(
        TableName=name, ConsistentRead=True,
        KeyConditions={"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}},
        QueryFilter={"n": {"ComparisonOperator": "GT", "AttributeValueList": [{"N": "25"}]},
                     "tag": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "blue"}]}},
        ConditionalOperator="OR"))
    snapshot.match("scan-filter", ddb.scan(
        TableName=name, ConsistentRead=True,
        ScanFilter={"tag": {"ComparisonOperator": "BEGINS_WITH", "AttributeValueList": [{"S": "bl"}]},
                    "gone": {"ComparisonOperator": "NULL"}}), unordered=("Items",))
    snapshot.match("scan-in", ddb.scan(
        TableName=name, ConsistentRead=True,
        ScanFilter={"n": {"ComparisonOperator": "IN", "AttributeValueList": [{"N": "10"}, {"N": "30"}]}}), unordered=("Items",))
    snapshot.match("batch-get", ddb.batch_get_item(RequestItems={name: {"Keys": [key], "AttributesToGet": ["tag"], "ConsistentRead": True}}))


def test_legacy_writes(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name = legacy_table(ddb, names, cleanup)
    key = {"pk": {"S": "a"}, "sk": {"N": "2"}}
    snapshot.error("put-expected-absent", lambda: ddb.put_item(
        TableName=name, Item=key, Expected={"sk": {"Exists": False}}))
    snapshot.match("update-attribute-updates", ddb.update_item(
        TableName=name, Key=key, ReturnValues="ALL_NEW",
        AttributeUpdates={"n": {"Action": "ADD", "Value": {"N": "5"}},
                          "tags": {"Action": "ADD", "Value": {"SS": ["x", "y"]}},
                          "tag": {"Action": "DELETE"},
                          "note": {"Value": {"S": "hi"}}}), unordered=("SS",))
    snapshot.match("update-delete-from-set", ddb.update_item(
        TableName=name, Key=key, ReturnValues="UPDATED_NEW",
        AttributeUpdates={"tags": {"Action": "DELETE", "Value": {"SS": ["x"]}}},
        Expected={"note": {"Value": {"S": "hi"}}}))
    absent = {"pk": {"S": "b"}, "sk": {"N": "9"}}
    snapshot.match("delete-action-on-absent-item", ddb.update_item(
        TableName=name, Key=absent, AttributeUpdates={"x": {"Action": "DELETE"}}))
    snapshot.match("absent-item-not-made", ddb.get_item(TableName=name, Key=absent, ConsistentRead=True))
    snapshot.match("delete-expected", ddb.delete_item(
        TableName=name, Key={"pk": {"S": "a"}, "sk": {"N": "1"}}, ReturnValues="ALL_OLD",
        Expected={"n": {"Value": {"N": "10"}}, "tag": {"Exists": True, "Value": {"S": "red"}}}))
    snapshot.error("put-expected-or", lambda: ddb.put_item(
        TableName=name, Item={"pk": {"S": "a"}, "sk": {"N": "3"}},
        Expected={"n": {"ComparisonOperator": "LT", "AttributeValueList": [{"N": "5"}]}, "tag": {"Exists": False}},
        ConditionalOperator="OR"))


def test_legacy_refusals(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name = legacy_table(ddb, names, cleanup)
    key = {"pk": {"S": "a"}, "sk": {"N": "1"}}
    eq_a = {"pk": {"ComparisonOperator": "EQ", "AttributeValueList": [{"S": "a"}]}}
    snapshot.error("mix-projection", lambda: ddb.get_item(
        TableName=name, Key=key, AttributesToGet=["n"], ProjectionExpression="n"))
    snapshot.outcome("mix-filter", lambda: ddb.query(
        TableName=name, KeyConditions=eq_a, FilterExpression="n > :v", ExpressionAttributeValues={":v": {"N": "1"}}))
    snapshot.error("mix-condition", lambda: ddb.put_item(
        TableName=name, Item=key, Expected={"n": {"Exists": False}}, ConditionExpression="attribute_not_exists(pk)"))
    snapshot.error("mix-update", lambda: ddb.update_item(
        TableName=name, Key=key, AttributeUpdates={"n": {"Value": {"N": "1"}}}, UpdateExpression="SET n = :v",
        ExpressionAttributeValues={":v": {"N": "1"}}))
    snapshot.error("legacy-with-names", lambda: ddb.scan(
        TableName=name, ScanFilter={"n": {"ComparisonOperator": "NULL"}}, ExpressionAttributeNames={"#n": "n"}))
    snapshot.error("wrong-count", lambda: ddb.query(
        TableName=name, KeyConditions={"pk": {"ComparisonOperator": "EQ", "AttributeValueList": []}}))
    snapshot.error("not-indexable", lambda: ddb.query(
        TableName=name, KeyConditions={"pk": {"ComparisonOperator": "NE", "AttributeValueList": [{"S": "a"}]}}))
    snapshot.error("begins-with-number", lambda: ddb.query(
        TableName=name, KeyConditions=eq_a,
        QueryFilter={"n": {"ComparisonOperator": "BEGINS_WITH", "AttributeValueList": [{"N": "1"}]}}))
    snapshot.error("operator-with-one", lambda: ddb.query(
        TableName=name, KeyConditions=eq_a, QueryFilter={"n": {"ComparisonOperator": "NULL"}}, ConditionalOperator="AND"))
    snapshot.error("exists-without-value", lambda: ddb.put_item(
        TableName=name, Item=key, Expected={"n": {"Exists": True}}))
    snapshot.error("value-with-exists-false", lambda: ddb.put_item(
        TableName=name, Item=key, Expected={"n": {"Exists": False, "Value": {"N": "1"}}}))
    snapshot.error("put-without-value", lambda: ddb.update_item(
        TableName=name, Key=key, AttributeUpdates={"n": {"Action": "PUT"}}))
    snapshot.error("attributes-to-get-with-select", lambda: ddb.scan(
        TableName=name, AttributesToGet=["n"], Select="ALL_ATTRIBUTES"))


# --- On-demand backups and restores (stage dynamodb-ops). A backup of a small on-demand table costs a fraction of a cent; a
# restore takes AWS minutes.

BACKUP_VOLATILE = ("BackupArn", "SourceBackupArn", "BackupCreationDateTime", "BackupSizeBytes", "RestoreDateTime",
                   "TableCreationDateTime", "TableSizeBytes", "ItemCount", "CreationDateTime", "LastUpdateToPayPerRequestDateTime")


def test_backup_lifecycle(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "v": {"S": "1"}})
    backup_name = names("backup")

    def backed_up():
        # AWS (recorded 2026-11-19): on a table just made, CreateBackup can answer ContinuousBackupsUnavailableException.
        try:
            return ddb.create_backup(TableName=name, BackupName=backup_name)
        except ddb.exceptions.ContinuousBackupsUnavailableException:
            raise AssertionError("backups still being enabled")
    made = eventually(backed_up, timeout=300, every=5)
    arn = made["BackupDetails"]["BackupArn"]
    snapshot.match("create", made, opaque=("BackupArn",), drop=BACKUP_VOLATILE[2:])

    def available():
        r = ddb.describe_backup(BackupArn=arn)
        assert r["BackupDescription"]["BackupDetails"]["BackupStatus"] == "AVAILABLE", "still creating"
        return r

    snapshot.match("describe", eventually(available, timeout=600, every=5), opaque=("BackupArn",), drop=BACKUP_VOLATILE[2:])
    snapshot.match("list", ddb.list_backups(TableName=name), opaque=("BackupArn",), drop=BACKUP_VOLATILE[2:])
    target = names("restored")
    restored = ddb.restore_table_from_backup(TargetTableName=target, BackupArn=arn)
    cleanup(ddb.delete_table, TableName=target)
    snapshot.match("restore", restored, opaque=("SourceBackupArn",), drop=BACKUP_VOLATILE[2:] + ("TableStatus", "RestoreInProgress"))
    ddb.get_waiter("table_exists").wait(TableName=target, WaiterConfig={"Delay": 10, "MaxAttempts": 180})
    snapshot.match("restored-item", ddb.get_item(TableName=target, Key={"pk": {"S": "a"}}, ConsistentRead=True))
    snapshot.error("restore-onto-existing", lambda: ddb.restore_table_from_backup(TargetTableName=target, BackupArn=arn))
    snapshot.match("delete", ddb.delete_backup(BackupArn=arn), opaque=("BackupArn",), drop=BACKUP_VOLATILE[2:])
    snapshot.known(arn, "backup-arn")  # the refusal quotes it
    snapshot.error("describe-deleted", lambda: ddb.describe_backup(BackupArn=arn))


def test_backup_refusals(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    account = client("sts").get_caller_identity()["Account"]
    missing = f"arn:aws:dynamodb:us-east-1:{account}:table/{names('absent')}/backup/01700000000000-abcdef12"
    snapshot.error("backup-of-absent-table", lambda: ddb.create_backup(TableName=names("absent"), BackupName="nope"))
    snapshot.error("describe-absent", lambda: ddb.describe_backup(BackupArn=missing))
    snapshot.error("delete-absent", lambda: ddb.delete_backup(BackupArn=missing))
    snapshot.error("restore-absent", lambda: ddb.restore_table_from_backup(TargetTableName=names("t"), BackupArn=missing))


def test_point_in_time_restore(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, _ = table(ddb, names, cleanup)
    target = names("restored")
    snapshot.error("not-enabled", lambda: ddb.restore_table_to_point_in_time(
        SourceTableName=name, TargetTableName=target, UseLatestRestorableTime=True))
    ddb.update_continuous_backups(TableName=name, PointInTimeRecoverySpecification={"PointInTimeRecoveryEnabled": True})
    snapshot.error("too-early", lambda: ddb.restore_table_to_point_in_time(
        SourceTableName=name, TargetTableName=target, RestoreDateTime=1_000_000_000))
    snapshot.error("absent-source", lambda: ddb.restore_table_to_point_in_time(
        SourceTableName=names("absent"), TargetTableName=target, UseLatestRestorableTime=True))
    restored = ddb.restore_table_to_point_in_time(SourceTableName=name, TargetTableName=target, UseLatestRestorableTime=True)
    cleanup(ddb.delete_table, TableName=target)
    snapshot.match("restore", restored, drop=BACKUP_VOLATILE + ("TableStatus", "RestoreInProgress"))
    ddb.get_waiter("table_exists").wait(TableName=target, WaiterConfig={"Delay": 10, "MaxAttempts": 180})


# --- Resource policies (stage dynamodb-ops). The policy denies Scan to everyone: harmless to the run, and DeleteResourcePolicy
# stays open to the account.

def scan_denied(account, table_arn):
    import json
    return json.dumps({"Version": "2012-10-17", "Statement": [{
        "Sid": "noscan", "Effect": "Deny", "Principal": {"AWS": f"arn:aws:iam::{account}:root"},
        "Action": "dynamodb:Scan", "Resource": table_arn}]})


def test_resource_policy_lifecycle(client, names, cleanup, snapshot, eventually):
    ddb = client("dynamodb")
    account = client("sts").get_caller_identity()["Account"]
    name, created = table(ddb, names, cleanup)
    arn = created["TableDescription"]["TableArn"]
    snapshot.error("get-none", lambda: ddb.get_resource_policy(ResourceArn=arn))
    put = ddb.put_resource_policy(ResourceArn=arn, Policy=scan_denied(account, arn), ExpectedRevisionId="NO_POLICY")
    snapshot.match("put", put, opaque=("RevisionId",))
    revision = snapshot.known(put["RevisionId"], "revision")  # the mismatch refusals quote it

    def stored():
        # AWS (recorded 2026-11-19): right after the put, GetResourcePolicy may still answer PolicyNotFoundException.
        try:
            r = ddb.get_resource_policy(ResourceArn=arn)
        except ddb.exceptions.PolicyNotFoundException:
            raise AssertionError("not applied yet")
        assert r["RevisionId"] == revision, "not applied yet"
        return r

    snapshot.match("get", eventually(stored, timeout=180, every=3), opaque=("RevisionId",))
    snapshot.match("put-same", ddb.put_resource_policy(ResourceArn=arn, Policy=scan_denied(account, arn)), opaque=("RevisionId",))
    snapshot.error("put-expecting-none", lambda: ddb.put_resource_policy(
        ResourceArn=arn, Policy=scan_denied(account, arn), ExpectedRevisionId="NO_POLICY"))
    snapshot.error("delete-wrong-revision", lambda: ddb.delete_resource_policy(ResourceArn=arn, ExpectedRevisionId="1"))
    snapshot.error("put-not-json", lambda: ddb.put_resource_policy(ResourceArn=arn, Policy="{not json"))
    snapshot.error("put-absent-table", lambda: ddb.put_resource_policy(
        ResourceArn=f"arn:aws:dynamodb:us-east-1:{account}:table/{names('absent')}", Policy=scan_denied(account, arn)))
    snapshot.match("delete", ddb.delete_resource_policy(ResourceArn=arn, ExpectedRevisionId=revision), opaque=("RevisionId",))


# --- Exports to and imports from S3 (stage dynamodb-ops). Point-in-time recovery, a small export and a small import cost cents at
# most; AWS takes minutes for each. The bucket is emptied and deleted after.

def export_bucket(s3, names, cleanup):
    name = names("bucket")
    s3.create_bucket(Bucket=name)

    def empty_and_delete():
        for page in s3.get_paginator("list_objects_v2").paginate(Bucket=name):
            for o in page.get("Contents", []):
                s3.delete_object(Bucket=name, Key=o["Key"])
        s3.delete_bucket(Bucket=name)

    cleanup(empty_and_delete)
    return name


def enable_pitr(ddb, name, eventually):
    """AWS (recorded 2026-11-19): on a table just made, UpdateContinuousBackups can answer ContinuousBackupsUnavailableException
    ('Backups are being enabled') for a while."""
    def on():
        try:
            return ddb.update_continuous_backups(
                TableName=name, PointInTimeRecoverySpecification={"PointInTimeRecoveryEnabled": True})
        except ddb.exceptions.ContinuousBackupsUnavailableException:
            raise AssertionError("backups still being enabled")
    return eventually(on, timeout=300, every=5)


EXPORT_VOLATILE = ("ExportArn", "ImportArn", "StartTime", "EndTime", "ExportTime", "ExportManifest", "BilledSizeBytes",
                   "ProcessedSizeBytes", "CloudWatchLogGroupArn", "TableId")


def test_export_and_import(client, names, cleanup, snapshot, eventually):
    ddb, s3 = client("dynamodb"), client("s3")
    name, created = table(ddb, names, cleanup)
    enable_pitr(ddb, name, eventually)
    for pk in ("a", "b"):
        ddb.put_item(TableName=name, Item={"pk": {"S": pk}, "n": {"N": "1"}})
    bucket = export_bucket(s3, names, cleanup)
    # AWS (recorded 2026-11-19): a prefix shorter than 3 characters is refused.
    snapshot.error("prefix-too-short", lambda: ddb.export_table_to_point_in_time(
        TableArn=created["TableDescription"]["TableArn"], S3Bucket=bucket, S3Prefix="x"))
    started = ddb.export_table_to_point_in_time(TableArn=created["TableDescription"]["TableArn"], S3Bucket=bucket, S3Prefix="exp")
    snapshot.match("export", started, drop=EXPORT_VOLATILE + ("ItemCount",))
    arn = started["ExportDescription"]["ExportArn"]

    def exported():
        r = ddb.describe_export(ExportArn=arn)
        assert r["ExportDescription"]["ExportStatus"] != "IN_PROGRESS", "still exporting"
        return r

    snapshot.match("exported", eventually(exported, timeout=1800, every=20), drop=EXPORT_VOLATILE)
    keys = sorted(o["Key"].split("/", 3)[-1] if "/data/" not in o["Key"] else "data"
                  for o in s3.list_objects_v2(Bucket=bucket)["Contents"])
    snapshot.match("layout", {"Keys": keys})
    data_prefix = next(o["Key"] for o in s3.list_objects_v2(Bucket=bucket)["Contents"] if "/data/" in o["Key"]).rsplit("/", 1)[0]
    target = names("imported")
    imported = ddb.import_table(
        S3BucketSource={"S3Bucket": bucket, "S3KeyPrefix": data_prefix + "/"}, InputFormat="DYNAMODB_JSON",
        InputCompressionType="GZIP",
        TableCreationParameters={"TableName": target, "BillingMode": "PAY_PER_REQUEST",
                                 "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                                 "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]})
    cleanup(ddb.delete_table, TableName=target)
    snapshot.match("import", imported, drop=EXPORT_VOLATILE)
    import_arn = imported["ImportTableDescription"]["ImportArn"]

    def done():
        r = ddb.describe_import(ImportArn=import_arn)
        # Settled either way: a FAILED import is AWS's answer too, and waiting for COMPLETED would sit out the timeout.
        assert r["ImportTableDescription"]["ImportStatus"] not in ("IN_PROGRESS", "CANCELLING"), "still importing"
        return r

    snapshot.match("imported", eventually(done, timeout=1800, every=20), drop=EXPORT_VOLATILE)
    snapshot.match("imported-item", ddb.get_item(TableName=target, Key={"pk": {"S": "b"}}, ConsistentRead=True))


def test_export_refusals(client, names, cleanup, snapshot):
    ddb = client("dynamodb")
    name, created = table(ddb, names, cleanup)
    arn = created["TableDescription"]["TableArn"]
    snapshot.error("recovery-off", lambda: ddb.export_table_to_point_in_time(TableArn=arn, S3Bucket=names("bucket")))
    snapshot.error("describe-absent", lambda: ddb.describe_export(ExportArn=f"{arn}/export/01700000000000-abcdef12"))
    snapshot.error("describe-absent-import", lambda: ddb.describe_import(ImportArn=f"{arn}/import/01700000000000-abcdef12"))


# --- Kinesis streaming destinations (stage dynamodb-ops). The Kinesis stream bills by the shard-hour (one shard, deleted at the
# end of the test: well under a cent).

def test_kinesis_streaming_destination(client, names, cleanup, snapshot, eventually):
    ddb, kinesis = client("dynamodb"), client("kinesis")
    name, _ = table(ddb, names, cleanup)
    stream_name = names("stream")
    kinesis.create_stream(StreamName=stream_name, ShardCount=1)
    cleanup(kinesis.delete_stream, StreamName=stream_name, EnforceConsumerDeletion=True)
    kinesis.get_waiter("stream_exists").wait(StreamName=stream_name, WaiterConfig=FAST)
    stream = kinesis.describe_stream_summary(StreamName=stream_name)["StreamDescriptionSummary"]["StreamARN"]
    snapshot.match("enable", ddb.enable_kinesis_streaming_destination(TableName=name, StreamArn=stream))

    def active():
        r = ddb.describe_kinesis_streaming_destination(TableName=name)
        assert r["KinesisDataStreamDestinations"][0]["DestinationStatus"] == "ACTIVE", "still enabling"
        return r

    snapshot.match("active", eventually(active, timeout=600, every=5))
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "v": {"N": "1"}})
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "v": {"N": "2"}})
    shard = kinesis.list_shards(StreamName=stream_name)["Shards"][0]["ShardId"]
    it = kinesis.get_shard_iterator(StreamName=stream_name, ShardId=shard, ShardIteratorType="TRIM_HORIZON")["ShardIterator"]
    got = []

    def both():
        nonlocal it
        r = kinesis.get_records(ShardIterator=it)
        got.extend(r["Records"])
        it = r["NextShardIterator"]
        assert len(got) >= 2, "the records are not all there yet"

    eventually(both, timeout=120, every=2)
    import json
    records = [json.loads(r["Data"]) for r in got]
    snapshot.match("records", {"Records": records}, drop=("eventID", "ApproximateCreationDateTime"))
    snapshot.error("enable-twice", lambda: ddb.enable_kinesis_streaming_destination(TableName=name, StreamArn=stream))
    snapshot.match("update", ddb.update_kinesis_streaming_destination(
        TableName=name, StreamArn=stream,
        UpdateKinesisStreamingConfiguration={"ApproximateCreationDateTimePrecision": "MICROSECOND"}))

    def settled():
        r = ddb.describe_kinesis_streaming_destination(TableName=name)
        assert r["KinesisDataStreamDestinations"][0]["DestinationStatus"] == "ACTIVE", "still updating"
        return r

    eventually(settled, timeout=600, every=5)
    snapshot.match("disable", ddb.disable_kinesis_streaming_destination(TableName=name, StreamArn=stream))
    snapshot.error("absent-table", lambda: ddb.describe_kinesis_streaming_destination(TableName=names("absent")))
