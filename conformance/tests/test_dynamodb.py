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
    arn = created["TableDescription"]["TableArn"]
    snapshot.match("tag", ddb.tag_resource(ResourceArn=arn, Tags=[{"Key": "team", "Value": "a"}, {"Key": "env", "Value": "dev"}]))

    def tagged():
        r = ddb.list_tags_of_resource(ResourceArn=arn)
        assert len(r["Tags"]) == 2, "tags are eventually consistent"
        return r

    snapshot.match("list", eventually(tagged), unordered=("Tags",))
    snapshot.match("untag", ddb.untag_resource(ResourceArn=arn, TagKeys=["env"]))
    snapshot.error("reserved-prefix", lambda: ddb.tag_resource(ResourceArn=arn, Tags=[{"Key": "aws:x", "Value": "a"}]))


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

    eventually(ready)
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

    snapshot.match("enabled", eventually(enabled), drop=("LastUpdateDateTime", "ContributorInsightsRuleList"))
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
    shard = described["StreamDescription"]["Shards"][0]["ShardId"]
    ddb.put_item(TableName=name, Item={"pk": {"S": "a"}, "n": {"N": "1"}})
    ddb.update_item(TableName=name, Key={"pk": {"S": "a"}}, UpdateExpression="SET n = :v",
                    ExpressionAttributeValues={":v": {"N": "2"}})
    ddb.delete_item(TableName=name, Key={"pk": {"S": "a"}})
    it = streams.get_shard_iterator(StreamArn=arn, ShardId=shard, ShardIteratorType="TRIM_HORIZON")["ShardIterator"]
    got = []

    def all_three():
        nonlocal it
        r = streams.get_records(ShardIterator=it)
        got.extend(r["Records"])
        it = r.get("NextShardIterator", it)
        assert len(got) >= 3, "the records are not all there yet"

    eventually(all_three)
    snapshot.match("records", {"Records": got}, drop=volatile)
    snapshot.error("unknown-shard", lambda: streams.get_shard_iterator(
        StreamArn=arn, ShardId="shardId-00000000000000000000-00000000", ShardIteratorType="LATEST"))
    snapshot.error("bad-iterator", lambda: streams.get_records(ShardIterator="x" * 40))
