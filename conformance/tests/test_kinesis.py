"""Kinesis Data Streams through boto3.

Cost on AWS: a provisioned shard is billed by the hour — about a cent and a
half — and each stream here lives for a minute or two.
"""

import time

FAST = {"Delay": 1, "MaxAttempts": 180}

# Positions and cursors the service mints.
MINTED = ("SequenceNumber", "StartingSequenceNumber", "EndingSequenceNumber",
          "ShardIterator", "NextShardIterator", "NextToken")
# How far behind the tip a read is.
WEATHER = ("MillisBehindLatest",)


def stream(kinesis, names, cleanup, shards=1):
    name = names("stream")
    kinesis.create_stream(StreamName=name, ShardCount=shards)
    cleanup(kinesis.delete_stream, StreamName=name, EnforceConsumerDeletion=True)
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    return name


def test_stream_lifecycle(client, names, cleanup, snapshot):
    kinesis = client("kinesis")
    name = names("stream")
    snapshot.match("create", kinesis.create_stream(StreamName=name, ShardCount=1))
    cleanup(kinesis.delete_stream, StreamName=name)
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)

    snapshot.match("summary", kinesis.describe_stream_summary(StreamName=name), drop=WEATHER)
    snapshot.match("shards", kinesis.list_shards(StreamName=name), opaque=MINTED)
    snapshot.match("tag", kinesis.add_tags_to_stream(StreamName=name, Tags={"env": "dev"}))
    snapshot.match("tags", kinesis.list_tags_for_stream(StreamName=name))
    snapshot.match("retention-up", kinesis.increase_stream_retention_period(
        StreamName=name, RetentionPeriodHours=48))
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    snapshot.match("summary-after", kinesis.describe_stream_summary(StreamName=name), drop=WEATHER)
    snapshot.match("delete", kinesis.delete_stream(StreamName=name))
    kinesis.get_waiter("stream_not_exists").wait(StreamName=name, WaiterConfig=FAST)
    snapshot.error("describe-deleted", lambda: kinesis.describe_stream_summary(StreamName=name))


def test_a_split_closes_the_parent_even_if_it_never_held_a_record(client, names, cleanup, snapshot, eventually, target):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)
    # ListShards can answer "not found" for a moment after the stream is ACTIVE on real AWS (recorded 2026-11-09).
    def first_shard():
        try:
            return kinesis.list_shards(StreamName=name)["Shards"][0]
        except kinesis.exceptions.ResourceNotFoundException as e:
            raise AssertionError(str(e))
    parent = eventually(first_shard)
    mid = int(parent["HashKeyRange"]["EndingHashKey"]) // 2
    snapshot.match("split", kinesis.split_shard(
        StreamName=name, ShardToSplit=parent["ShardId"], NewStartingHashKey=str(mid)))
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    # Whether and when AWS closes the parent of a shard that never held a record is the question, so poll briefly and record the answer.
    deadline = time.time() + (45 if target.name == "aws" else 5)
    while True:
        shards = kinesis.list_shards(StreamName=name)["Shards"]
        closed = [s for s in shards if s["ShardId"] == parent["ShardId"]][0]
        if "EndingSequenceNumber" in closed["SequenceNumberRange"] or time.time() > deadline:
            break
        time.sleep(1)
    snapshot.match("parent-closed", {"closed": "EndingSequenceNumber" in closed["SequenceNumberRange"]})
    snapshot.match("shards", shards, opaque=MINTED)


def test_records_are_read_back_in_order(client, names, cleanup, snapshot, eventually):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)

    snapshot.match("put", kinesis.put_record(StreamName=name, Data=b"one", PartitionKey="k"),
                   opaque=MINTED)
    snapshot.match("put-many", kinesis.put_records(StreamName=name, Records=[
        {"Data": b"two", "PartitionKey": "k"}, {"Data": b"three", "PartitionKey": "k"}]),
        opaque=MINTED)

    shard = kinesis.list_shards(StreamName=name)["Shards"][0]["ShardId"]
    it = snapshot.match("iterator", kinesis.get_shard_iterator(
        StreamName=name, ShardId=shard, ShardIteratorType="TRIM_HORIZON"), opaque=MINTED)
    got = []
    cursor = [it["ShardIterator"]]

    def read():
        r = kinesis.get_records(ShardIterator=cursor[0], Limit=10)
        cursor[0] = r["NextShardIterator"]
        got.extend(r["Records"])
        assert len(got) >= 3, f"{len(got)} of 3 records so far"
        return got

    snapshot.match("records", eventually(read, timeout=60), opaque=MINTED)

    latest = kinesis.get_shard_iterator(
        StreamName=name, ShardId=shard, ShardIteratorType="LATEST")["ShardIterator"]
    snapshot.match("nothing-after-latest", kinesis.get_records(ShardIterator=latest),
                   opaque=MINTED, drop=WEATHER)


def test_partition_keys_route_across_shards(client, names, cleanup, snapshot):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup, shards=2)
    snapshot.match("shards", kinesis.list_shards(StreamName=name), opaque=MINTED)
    # Where a key lands is MD5 of the key against the hash ranges: fixed, and
    # the same on any stream with two even shards.
    placed = {}
    for k in ("a", "b", "c", "d", "user-1", "user-2"):
        placed[k] = kinesis.put_record(StreamName=name, Data=b"x", PartitionKey=k)["ShardId"]
    snapshot.match("routing", placed)
    snapshot.match("explicit-hash-key", kinesis.put_record(
        StreamName=name, Data=b"x", PartitionKey="a", ExplicitHashKey="0"), opaque=MINTED)


def test_refusals(client, names, cleanup, snapshot):
    kinesis = client("kinesis")
    absent = names("absent")
    snapshot.error("describe-absent", lambda: kinesis.describe_stream_summary(StreamName=absent))
    snapshot.error("put-to-absent", lambda: kinesis.put_record(
        StreamName=absent, Data=b"x", PartitionKey="k"))
    snapshot.error("delete-absent", lambda: kinesis.delete_stream(StreamName=absent))

    name = stream(kinesis, names, cleanup)
    snapshot.error("create-twice", lambda: kinesis.create_stream(StreamName=name, ShardCount=1))
    snapshot.error("absent-shard", lambda: kinesis.get_shard_iterator(
        StreamName=name, ShardId="shardId-000000000099", ShardIteratorType="LATEST"))
    snapshot.error("garbage-iterator", lambda: kinesis.get_records(ShardIterator="not-an-iterator"))
    snapshot.error("at-sequence-without-one", lambda: kinesis.get_shard_iterator(
        StreamName=name, ShardId="shardId-000000000000", ShardIteratorType="AT_SEQUENCE_NUMBER"))
    snapshot.error("bad-explicit-hash-key", lambda: kinesis.put_record(
        StreamName=name, Data=b"x", PartitionKey="k", ExplicitHashKey="not-a-number"))
    snapshot.error("retention-below-current", lambda: kinesis.decrease_stream_retention_period(
        StreamName=name, RetentionPeriodHours=12))
    # "Increase" to what it already is: refused or a no-op is AWS's to say.
    snapshot.outcome("retention-not-an-increase", lambda: kinesis.increase_stream_retention_period(
        StreamName=name, RetentionPeriodHours=24))


# Written ahead of a recording (2026-11-12): UNVERIFIED until recorded against AWS.

def test_every_iterator_type(client, names, cleanup, snapshot, eventually):
    import datetime
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)
    first = kinesis.put_record(StreamName=name, Data=b"a", PartitionKey="k")["SequenceNumber"]
    before = time.time()
    time.sleep(2)
    kinesis.put_record(StreamName=name, Data=b"b", PartitionKey="k")
    kinesis.put_record(StreamName=name, Data=b"c", PartitionKey="k")
    shard = kinesis.list_shards(StreamName=name)["Shards"][0]["ShardId"]

    def read(**kw):
        it = kinesis.get_shard_iterator(StreamName=name, ShardId=shard, **kw)["ShardIterator"]

        def poll():
            r = kinesis.get_records(ShardIterator=it)
            assert r["Records"], "nothing yet"
            return [x["Data"] for x in r["Records"]]
        return eventually(poll, timeout=60)

    snapshot.match("at-sequence", read(ShardIteratorType="AT_SEQUENCE_NUMBER", StartingSequenceNumber=first))
    snapshot.match("after-sequence", read(ShardIteratorType="AFTER_SEQUENCE_NUMBER", StartingSequenceNumber=first))
    snapshot.match("at-timestamp", read(ShardIteratorType="AT_TIMESTAMP",
                                        Timestamp=datetime.datetime.fromtimestamp(before + 1, datetime.timezone.utc)))


def test_consumers_policy_and_tags_by_arn(client, names, cleanup, snapshot):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)
    arn = kinesis.describe_stream_summary(StreamName=name)["StreamDescriptionSummary"]["StreamARN"]
    made = kinesis.register_stream_consumer(StreamARN=arn, ConsumerName="c1")
    snapshot.match("register", made, opaque=("ConsumerARN", "ConsumerCreationTimestamp"), drop=("ConsumerStatus",))
    snapshot.match("list-consumers", kinesis.list_stream_consumers(StreamARN=arn),
                   opaque=("ConsumerARN", "ConsumerCreationTimestamp"), drop=("ConsumerStatus",))
    snapshot.error("delete-with-consumers", lambda: kinesis.delete_stream(StreamName=name))
    kinesis.deregister_stream_consumer(StreamARN=arn, ConsumerName="c1")
    snapshot.match("tag-by-arn", kinesis.tag_resource(ResourceARN=arn, Tags={"env": "dev"}))
    snapshot.match("tags-by-arn", kinesis.list_tags_for_resource(ResourceARN=arn))
    snapshot.error("no-policy", lambda: kinesis.get_resource_policy(ResourceARN=arn))


# Written ahead of a recording (2026-11-13): UNVERIFIED until recorded against AWS.

def first_shards(kinesis, name, count, eventually):
    # ListShards can answer "not found" for a moment after the stream is ACTIVE on real AWS (recorded 2026-11-09).
    def listed():
        try:
            shards = kinesis.list_shards(StreamName=name)["Shards"]
        except kinesis.exceptions.ResourceNotFoundException as e:
            raise AssertionError(str(e))
        assert len(shards) == count, f"{len(shards)} of {count} shards so far"
        return shards
    return eventually(listed)


def drain(kinesis, name, shard, eventually):
    """Reads a closed shard to its end: the data it held, and the children it hands over to."""
    it = [kinesis.get_shard_iterator(StreamName=name, ShardId=shard,
                                     ShardIteratorType="TRIM_HORIZON")["ShardIterator"]]
    got = []
    children = []

    def read():
        for _ in range(20):
            r = kinesis.get_records(ShardIterator=it[0])
            got.extend(x["Data"] for x in r["Records"])
            if "NextShardIterator" not in r:
                children[:] = r["ChildShards"]
                return {"Data": got, "ChildShards": children}
            it[0] = r["NextShardIterator"]
        raise AssertionError("the shard has not ended yet")

    return eventually(read, timeout=120)


def test_a_merge_names_both_parents_and_a_closed_shard_reads_to_its_end(client, names, cleanup, snapshot, eventually):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup, shards=3)
    s0, s1, s2 = first_shards(kinesis, name, 3, eventually)
    for shard, data in ((s0, b"zero"), (s1, b"one")):
        kinesis.put_record(StreamName=name, Data=data, PartitionKey="k",
                           ExplicitHashKey=shard["HashKeyRange"]["StartingHashKey"])
    snapshot.error("merge-a-gap", lambda: kinesis.merge_shards(
        StreamName=name, ShardToMerge=s0["ShardId"], AdjacentShardToMerge=s2["ShardId"]))
    snapshot.error("merge-with-itself", lambda: kinesis.merge_shards(
        StreamName=name, ShardToMerge=s0["ShardId"], AdjacentShardToMerge=s0["ShardId"]))
    snapshot.error("merge-an-absent-shard", lambda: kinesis.merge_shards(
        StreamName=name, ShardToMerge=s0["ShardId"], AdjacentShardToMerge="shardId-000000000099"))
    snapshot.match("merge", kinesis.merge_shards(
        StreamName=name, ShardToMerge=s0["ShardId"], AdjacentShardToMerge=s1["ShardId"]))
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    shards = first_shards(kinesis, name, 4, eventually)
    snapshot.match("shards", shards, opaque=MINTED)
    assert shards[3]["ParentShardId"] == s0["ShardId"]
    assert shards[3]["AdjacentParentShardId"] == s1["ShardId"]
    snapshot.match("first-parent-drained", drain(kinesis, name, s0["ShardId"], eventually), opaque=MINTED)
    snapshot.match("second-parent-drained", drain(kinesis, name, s1["ShardId"], eventually), opaque=MINTED)
    snapshot.error("merge-a-closed-shard", lambda: kinesis.merge_shards(
        StreamName=name, ShardToMerge=s0["ShardId"], AdjacentShardToMerge=s1["ShardId"]))


def test_a_split_hands_a_records_shard_over_to_its_children(client, names, cleanup, snapshot, eventually):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)
    (parent,) = first_shards(kinesis, name, 1, eventually)
    kinesis.put_record(StreamName=name, Data=b"before", PartitionKey="k", ExplicitHashKey="0")
    mid = (int(parent["HashKeyRange"]["EndingHashKey"]) + 1) // 2
    snapshot.error("split-outside-the-shard", lambda: kinesis.split_shard(
        StreamName=name, ShardToSplit=parent["ShardId"], NewStartingHashKey="0"))
    snapshot.match("split", kinesis.split_shard(
        StreamName=name, ShardToSplit=parent["ShardId"], NewStartingHashKey=str(mid)))
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    snapshot.error("split-a-closed-shard", lambda: kinesis.split_shard(
        StreamName=name, ShardToSplit=parent["ShardId"], NewStartingHashKey=str(mid)))
    snapshot.match("parent-drained", drain(kinesis, name, parent["ShardId"], eventually), opaque=MINTED)
    # Only the open shards, and the filtered listing the KCL uses.
    snapshot.match("open-shards", kinesis.list_shards(
        StreamName=name, ShardFilter={"Type": "AT_LATEST"}), opaque=MINTED)
    placed = kinesis.put_record(StreamName=name, Data=b"after", PartitionKey="k", ExplicitHashKey=str(mid))
    snapshot.match("routing-after", {"ShardId": placed["ShardId"]})


def test_a_new_shard_count_re_tiles_the_stream(client, names, cleanup, snapshot, eventually):
    kinesis = client("kinesis")
    name = stream(kinesis, names, cleanup)
    first_shards(kinesis, name, 1, eventually)
    snapshot.match("scale-up", kinesis.update_shard_count(
        StreamName=name, TargetShardCount=2, ScalingType="UNIFORM_SCALING"))
    kinesis.get_waiter("stream_exists").wait(StreamName=name, WaiterConfig=FAST)
    snapshot.match("shards", first_shards(kinesis, name, 3, eventually), opaque=MINTED)
    snapshot.error("scale-up-past-double", lambda: kinesis.update_shard_count(
        StreamName=name, TargetShardCount=5, ScalingType="UNIFORM_SCALING"))
    snapshot.match("summary", kinesis.describe_stream_summary(StreamName=name), drop=WEATHER)


def test_account_settings(client, snapshot):
    # Only read: enabling a commitment on a real account would bill it for a day.
    kinesis = client("kinesis")
    snapshot.match("describe", kinesis.describe_account_settings())
