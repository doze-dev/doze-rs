"""S3 through boto3 — with the SDK's own defaults, which since botocore 1.36
means a CRC32 on every upload and checksum validation on every download."""

MB = 1024 * 1024

# The canonical user of the account: real on AWS, a constant here, and no part
# of what a bucket does.
OWNER = ("Owner", "Initiator")


def bucket(s3, names, cleanup, label="bucket"):
    name = names(label)
    s3.create_bucket(Bucket=name)

    def empty_and_delete():
        for page in s3.get_paginator("list_object_versions").paginate(Bucket=name):
            for v in page.get("Versions", []) + page.get("DeleteMarkers", []):
                s3.delete_object(Bucket=name, Key=v["Key"], VersionId=v["VersionId"])
        s3.delete_bucket(Bucket=name)

    cleanup(empty_and_delete)
    return name


def test_bucket_and_object_lifecycle(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = names("bucket")
    snapshot.match("create-bucket", s3.create_bucket(Bucket=name))
    cleanup(s3.delete_bucket, Bucket=name)
    cleanup(s3.delete_object, Bucket=name, Key="greeting.txt")

    snapshot.match("location", s3.get_bucket_location(Bucket=name))
    snapshot.match("empty-listing", s3.list_objects_v2(Bucket=name))

    snapshot.match("put", s3.put_object(
        Bucket=name, Key="greeting.txt", Body=b"hello", ContentType="text/plain",
        Metadata={"origin": "conformance"}))
    snapshot.match("head", s3.head_object(Bucket=name, Key="greeting.txt"))
    snapshot.match("get", s3.get_object(Bucket=name, Key="greeting.txt"))
    snapshot.match("listing", s3.list_objects_v2(Bucket=name))

    snapshot.match("copy", s3.copy_object(
        Bucket=name, Key="copy.txt", CopySource={"Bucket": name, "Key": "greeting.txt"}))
    cleanup(s3.delete_object, Bucket=name, Key="copy.txt")

    snapshot.match("delete", s3.delete_object(Bucket=name, Key="greeting.txt"))
    snapshot.match("delete-again", s3.delete_object(Bucket=name, Key="greeting.txt"))


def test_ranged_reads(client, names, cleanup, snapshot):
    """A ranged read of an object that carries a checksum — which, with the
    SDK defaults, is every object boto3 uploaded. The SDK validates whatever
    checksum header comes back against the bytes it was actually sent."""
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    s3.put_object(Bucket=name, Key="k", Body=b"hello world")

    snapshot.match("middle", s3.get_object(Bucket=name, Key="k", Range="bytes=1-3"))
    snapshot.match("suffix", s3.get_object(Bucket=name, Key="k", Range="bytes=-5"))
    snapshot.match("open-ended", s3.get_object(Bucket=name, Key="k", Range="bytes=6-"))
    snapshot.match("whole-by-range", s3.get_object(Bucket=name, Key="k", Range="bytes=0-10"))
    snapshot.error("past-the-end", lambda: s3.get_object(Bucket=name, Key="k", Range="bytes=50-60"))


def test_listing_with_prefix_delimiter_and_pages(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    for key in ("a/1", "a/2", "b/1", "b/c/1", "top"):
        s3.put_object(Bucket=name, Key=key, Body=key.encode())

    snapshot.match("folders", s3.list_objects_v2(Bucket=name, Delimiter="/"))
    snapshot.match("under-b", s3.list_objects_v2(Bucket=name, Prefix="b/", Delimiter="/"))
    first = snapshot.match("page-1", s3.list_objects_v2(Bucket=name, MaxKeys=2),
                           opaque=("NextContinuationToken",))
    snapshot.match("page-2", s3.list_objects_v2(
        Bucket=name, MaxKeys=2, ContinuationToken=first["NextContinuationToken"]),
        opaque=("NextContinuationToken", "ContinuationToken"))
    snapshot.match("start-after", s3.list_objects_v2(Bucket=name, StartAfter="b/1"))
    snapshot.match("v1-listing", s3.list_objects(Bucket=name, Delimiter="/"), drop=OWNER)


def test_versioning_and_delete_markers(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    snapshot.match("unversioned", s3.get_bucket_versioning(Bucket=name))
    snapshot.match("enable", s3.put_bucket_versioning(
        Bucket=name, VersioningConfiguration={"Status": "Enabled"}))
    snapshot.match("enabled", s3.get_bucket_versioning(Bucket=name))

    v = ("VersionId",)
    one = snapshot.match("put-v1", s3.put_object(Bucket=name, Key="k", Body=b"one"), opaque=v)
    snapshot.match("put-v2", s3.put_object(Bucket=name, Key="k", Body=b"two"), opaque=v)
    snapshot.match("delete", s3.delete_object(Bucket=name, Key="k"), opaque=v)

    snapshot.error("get-latest-is-a-marker", lambda: s3.get_object(Bucket=name, Key="k"))
    snapshot.match("get-v1", s3.get_object(Bucket=name, Key="k", VersionId=one["VersionId"]),
                   opaque=v)
    snapshot.match("versions", s3.list_object_versions(Bucket=name), opaque=v, drop=OWNER)


def test_multipart_upload(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    # NextUploadIdMarker is the id of the last upload listed: as opaque as the id (the first recording kept AWS's raw value, which no
    # other run can equal; rewritten offline to the token on 2026-11-12).
    u = ("UploadId", "NextUploadIdMarker")

    started = snapshot.match("create", s3.create_multipart_upload(Bucket=name, Key="big"), opaque=u)
    uid = started["UploadId"]
    # Every part but the last must be at least 5 MiB.
    p1 = snapshot.match("part-1", s3.upload_part(
        Bucket=name, Key="big", UploadId=uid, PartNumber=1, Body=b"a" * (5 * MB)))
    p2 = snapshot.match("part-2", s3.upload_part(
        Bucket=name, Key="big", UploadId=uid, PartNumber=2, Body=b"b" * 10))
    snapshot.match("parts", s3.list_parts(Bucket=name, Key="big", UploadId=uid),
                   opaque=u, drop=OWNER)
    snapshot.match("in-progress", s3.list_multipart_uploads(Bucket=name), opaque=u, drop=OWNER)

    snapshot.match("complete", s3.complete_multipart_upload(
        Bucket=name, Key="big", UploadId=uid, MultipartUpload={"Parts": [
            {"PartNumber": 1, "ETag": p1["ETag"]},
            {"PartNumber": 2, "ETag": p2["ETag"]},
        ]}))
    snapshot.match("head", s3.head_object(Bucket=name, Key="big"))
    snapshot.match("get", s3.get_object(Bucket=name, Key="big"))

    small = s3.create_multipart_upload(Bucket=name, Key="small")["UploadId"]
    a = s3.upload_part(Bucket=name, Key="small", UploadId=small, PartNumber=1, Body=b"tiny")
    b = s3.upload_part(Bucket=name, Key="small", UploadId=small, PartNumber=2, Body=b"tiny")
    snapshot.error("part-too-small", lambda: s3.complete_multipart_upload(
        Bucket=name, Key="small", UploadId=small, MultipartUpload={"Parts": [
            {"PartNumber": 1, "ETag": a["ETag"]}, {"PartNumber": 2, "ETag": b["ETag"]}]}),
        drop=("UploadId",))
    snapshot.match("abort", s3.abort_multipart_upload(Bucket=name, Key="small", UploadId=small))


def test_refusals(client, names, cleanup, snapshot):
    s3 = client("s3")
    absent = names("absent")
    snapshot.error("no-such-bucket", lambda: s3.list_objects_v2(Bucket=absent))
    snapshot.error("head-no-such-bucket", lambda: s3.head_bucket(Bucket=absent))
    snapshot.error("bad-bucket-name", lambda: s3.create_bucket(Bucket="Not_A_Bucket"))

    name = bucket(s3, names, cleanup)
    snapshot.error("no-such-key", lambda: s3.get_object(Bucket=name, Key="absent"))
    snapshot.error("head-no-such-key", lambda: s3.head_object(Bucket=name, Key="absent"))
    # Refused everywhere except us-east-1, where re-creating a bucket you own
    # succeeds. Whichever the recording region does, do that.
    snapshot.outcome("create-twice", lambda: s3.create_bucket(Bucket=name))

    s3.put_object(Bucket=name, Key="k", Body=b"x")
    snapshot.error("not-empty", lambda: s3.delete_bucket(Bucket=name))
    snapshot.error("precondition", lambda: s3.get_object(
        Bucket=name, Key="k", IfMatch='"0000"'))
    snapshot.error("no-such-upload", lambda: s3.upload_part(
        Bucket=name, Key="k", UploadId="absent", PartNumber=1, Body=b"x"))


def test_tagging_and_batch_delete(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    for key in ("a", "b", "c"):
        s3.put_object(Bucket=name, Key=key, Body=b"x")

    tags = {"TagSet": [{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": "a"}]}
    snapshot.match("tag-object", s3.put_object_tagging(Bucket=name, Key="a", Tagging=tags))
    snapshot.match("object-tags", s3.get_object_tagging(Bucket=name, Key="a"), unordered=("TagSet",))
    snapshot.match("head-counts-tags", s3.get_object(Bucket=name, Key="a"))
    snapshot.match("untag-object", s3.delete_object_tagging(Bucket=name, Key="a"))
    snapshot.match("no-object-tags", s3.get_object_tagging(Bucket=name, Key="a"))

    snapshot.error("no-bucket-tags", lambda: s3.get_bucket_tagging(Bucket=name))
    snapshot.match("tag-bucket", s3.put_bucket_tagging(Bucket=name, Tagging=tags))
    snapshot.match("bucket-tags", s3.get_bucket_tagging(Bucket=name), unordered=("TagSet",))

    snapshot.match("delete-many", s3.delete_objects(Bucket=name, Delete={
        "Objects": [{"Key": "a"}, {"Key": "b"}, {"Key": "never-existed"}]}),
        unordered=("Deleted",))
    snapshot.match("delete-quietly", s3.delete_objects(Bucket=name, Delete={
        "Objects": [{"Key": "c"}], "Quiet": True}))
    snapshot.match("empty", s3.list_objects_v2(Bucket=name))


def test_presigned_urls(client, names, cleanup, snapshot):
    """A URL boto3 signs and something that is not boto3 uses."""
    import urllib.error
    import urllib.request

    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    s3.put_object(Bucket=name, Key="k", Body=b"signed read", ContentType="text/plain")

    def fetch(url, method="GET", data=None):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, data=data, method=method)) as r:
                return {"status": r.status, "body": r.read().decode()}
        except urllib.error.HTTPError as e:
            return {"status": e.code}

    get = s3.generate_presigned_url("get_object", Params={"Bucket": name, "Key": "k"}, ExpiresIn=300)
    snapshot.match("get", fetch(get))
    put = s3.generate_presigned_url("put_object", Params={"Bucket": name, "Key": "up"}, ExpiresIn=300)
    snapshot.match("put", fetch(put, method="PUT", data=b"signed write"))
    # boto3 signs a checksum of the empty body into the URL, so on real AWS this PUT may be refused and write nothing (recorded 2026-11-09).
    snapshot.outcome("what-the-put-wrote", lambda: s3.get_object(Bucket=name, Key="up"))
    absent = s3.generate_presigned_url("get_object", Params={"Bucket": name, "Key": "absent"})
    snapshot.match("get-absent", fetch(absent))


def test_bucket_configuration(client, names, cleanup, snapshot, target):
    import json

    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    snapshot.error("no-cors", lambda: s3.get_bucket_cors(Bucket=name))
    snapshot.error("no-policy", lambda: s3.get_bucket_policy(Bucket=name))
    snapshot.error("no-lifecycle", lambda: s3.get_bucket_lifecycle_configuration(Bucket=name))
    snapshot.error("no-website", lambda: s3.get_bucket_website(Bucket=name))
    snapshot.match("default-encryption", s3.get_bucket_encryption(Bucket=name))
    snapshot.match("default-public-access-block", s3.get_public_access_block(Bucket=name))

    cors = {"CORSRules": [{"AllowedMethods": ["GET"], "AllowedOrigins": ["https://example.com"],
                           "AllowedHeaders": ["*"], "MaxAgeSeconds": 300}]}
    snapshot.match("put-cors", s3.put_bucket_cors(Bucket=name, CORSConfiguration=cors))
    snapshot.match("cors", s3.get_bucket_cors(Bucket=name))
    snapshot.match("delete-cors", s3.delete_bucket_cors(Bucket=name))

    # Not a public policy: a new bucket blocks those, here and on AWS.
    policy = {"Version": "2012-10-17", "Statement": [{
        "Sid": "own-account", "Effect": "Allow",
        "Principal": {"AWS": f"arn:aws:iam::{target.account}:root"},
        "Action": "s3:GetObject", "Resource": f"arn:aws:s3:::{name}/*"}]}
    snapshot.match("put-policy", s3.put_bucket_policy(Bucket=name, Policy=json.dumps(policy)))
    snapshot.match("policy", json.loads(s3.get_bucket_policy(Bucket=name)["Policy"]))
    snapshot.error("policy-for-another-bucket", lambda: s3.put_bucket_policy(
        Bucket=name, Policy=json.dumps({**policy, "Statement": [
            {**policy["Statement"][0], "Resource": "arn:aws:s3:::someone-elses-bucket/*"}]})))
    snapshot.error("policy-not-json", lambda: s3.put_bucket_policy(Bucket=name, Policy="{nope"))
    snapshot.match("delete-policy", s3.delete_bucket_policy(Bucket=name))

    lifecycle = {"Rules": [{"ID": "expire-tmp", "Status": "Enabled", "Filter": {"Prefix": "tmp/"},
                            "Expiration": {"Days": 7}}]}
    snapshot.match("put-lifecycle", s3.put_bucket_lifecycle_configuration(
        Bucket=name, LifecycleConfiguration=lifecycle))
    snapshot.match("lifecycle", s3.get_bucket_lifecycle_configuration(Bucket=name))


def test_copy_replaces_or_keeps_metadata(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    s3.put_object(Bucket=name, Key="src", Body=b"x", ContentType="text/plain",
                  Metadata={"origin": "first"}, CacheControl="max-age=60")
    src = {"Bucket": name, "Key": "src"}

    s3.copy_object(Bucket=name, Key="kept", CopySource=src)
    snapshot.match("kept", s3.head_object(Bucket=name, Key="kept"))
    s3.copy_object(Bucket=name, Key="replaced", CopySource=src, MetadataDirective="REPLACE",
                   ContentType="application/json", Metadata={"origin": "second"})
    snapshot.match("replaced", s3.head_object(Bucket=name, Key="replaced"))
    snapshot.error("onto-itself-unchanged", lambda: s3.copy_object(
        Bucket=name, Key="src", CopySource=src))
    snapshot.error("absent-source", lambda: s3.copy_object(
        Bucket=name, Key="x", CopySource={"Bucket": name, "Key": "absent"}))


def test_notification_fifo_queue(client, names, cleanup, snapshot):
    """SQS plan section 9 / S3-5, S3-26. "Amazon SQS FIFO queues aren't
    supported" as an event notification destination. Both queues carry a policy
    that lets this bucket send, so the difference between them is the type: the
    standard queue is the control, and the FIFO queue is refused (the code and
    text are recorded)."""
    import json

    s3, sqs = client("s3"), client("sqs")
    name = bucket(s3, names, cleanup)

    def queue(label, suffix, **attrs):
        url = sqs.create_queue(QueueName=names(label, suffix=suffix), Attributes=attrs)["QueueUrl"]
        cleanup(sqs.delete_queue, QueueUrl=url)
        arn = sqs.get_queue_attributes(QueueUrl=url, AttributeNames=["QueueArn"])["Attributes"]["QueueArn"]
        sqs.set_queue_attributes(QueueUrl=url, Attributes={"Policy": json.dumps({
            "Version": "2012-10-17",
            "Statement": [{"Effect": "Allow", "Principal": {"Service": "s3.amazonaws.com"},
                           "Action": "sqs:SendMessage", "Resource": arn,
                           "Condition": {"ArnLike": {"aws:SourceArn": f"arn:aws:s3:::{name}"}}}]})})
        return arn

    def put(arn):
        return s3.put_bucket_notification_configuration(Bucket=name, NotificationConfiguration={
            "QueueConfigurations": [{"QueueArn": arn, "Events": ["s3:ObjectCreated:*"]}]})

    standard = queue("standard", "")
    fifo = queue("fifo", ".fifo", FifoQueue="true")
    snapshot.match("standard-queue", put(standard))
    snapshot.error("fifo-queue", lambda: put(fifo))
    snapshot.match("configuration-after", s3.get_bucket_notification_configuration(Bucket=name),
                   opaque=("Id", "QueueArn"))


# ---------------------------------------------------------------------------------------------------------------------------------
# Scenarios written ahead of a recording (2026-11-12): every label below is UNVERIFIED until it is recorded against AWS. They pin the
# behaviour the Rust edition built from the S3 documentation, so a recording can confirm or correct it label by label.
# ---------------------------------------------------------------------------------------------------------------------------------


def test_keys_with_awkward_characters_survive_listing_and_reading(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    keys = ["with space", "plus+sign", "ünïcode/ключ", "a/b/c", "trailing/", "percent%25", "q?mark", "tilde~"]
    for k in keys:
        s3.put_object(Bucket=name, Key=k, Body=k.encode())
    listed = [o["Key"] for o in s3.list_objects_v2(Bucket=name)["Contents"]]
    assert listed == sorted(keys, key=lambda k: k.encode("utf-8"))
    for k in keys:
        assert s3.get_object(Bucket=name, Key=k)["Body"].read() == k.encode()
    snapshot.match("delimited", s3.list_objects_v2(Bucket=name, Delimiter="/"), drop=OWNER)


def test_listing_pages_across_delimiters(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    for i in range(30):
        s3.put_object(Bucket=name, Key=f"d{i % 3}/k{i:02}", Body=b"x")
    seen, token = [], None
    while True:
        kw = {"ContinuationToken": token} if token else {}
        page = s3.list_objects_v2(Bucket=name, Delimiter="/", MaxKeys=2, **kw)
        seen += [p["Prefix"] for p in page.get("CommonPrefixes", [])] + [o["Key"] for o in page.get("Contents", [])]
        token = page.get("NextContinuationToken")
        if not token:
            break
    assert seen == ["d0/", "d1/", "d2/"]
    under, token = [], None
    while True:
        kw = {"ContinuationToken": token} if token else {}
        page = s3.list_objects_v2(Bucket=name, Prefix="d1/", MaxKeys=4, **kw)
        under += [o["Key"] for o in page["Contents"]]
        token = page.get("NextContinuationToken")
        if not token:
            break
    assert under == [f"d1/k{i:02}" for i in range(1, 30, 3)]
    snapshot.match("empty-prefix", s3.list_objects_v2(Bucket=name, Prefix="nothing/"))


def test_versions_delete_markers_and_removing_a_version(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    s3.put_bucket_versioning(Bucket=name, VersioningConfiguration={"Status": "Enabled"})
    ids = [s3.put_object(Bucket=name, Key="k", Body=str(i).encode())["VersionId"] for i in range(3)]
    marker = s3.delete_object(Bucket=name, Key="k")
    assert marker["DeleteMarker"] is True
    listed = s3.list_object_versions(Bucket=name)
    assert [v["VersionId"] for v in listed["Versions"]] == ids[::-1]
    assert listed["DeleteMarkers"][0]["IsLatest"] is True
    s3.delete_object(Bucket=name, Key="k", VersionId=marker["VersionId"])
    assert s3.get_object(Bucket=name, Key="k")["Body"].read() == b"2"
    removed = snapshot.match("remove-version", s3.delete_object(Bucket=name, Key="k", VersionId=ids[1]), opaque=("VersionId",))
    assert removed["VersionId"] == ids[1]
    assert [v["VersionId"] for v in s3.list_object_versions(Bucket=name)["Versions"]] == [ids[2], ids[0]]
    s3.put_bucket_versioning(Bucket=name, VersioningConfiguration={"Status": "Suspended"})
    s3.put_object(Bucket=name, Key="k", Body=b"null-version")
    s3.put_object(Bucket=name, Key="k", Body=b"replaces-null")
    ver = {v["VersionId"] for v in s3.list_object_versions(Bucket=name)["Versions"]}
    assert "null" in ver and len(ver) == 3
    snapshot.match("suspended", s3.get_bucket_versioning(Bucket=name))


def test_multipart_parts_are_replaced_listed_and_aborted(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    uid = s3.create_multipart_upload(Bucket=name, Key="m")["UploadId"]
    first = s3.upload_part(Bucket=name, Key="m", UploadId=uid, PartNumber=1, Body=b"a" * (5 * MB))
    again = s3.upload_part(Bucket=name, Key="m", UploadId=uid, PartNumber=1, Body=b"b" * (5 * MB))
    assert first["ETag"] != again["ETag"]
    s3.upload_part(Bucket=name, Key="m", UploadId=uid, PartNumber=3, Body=b"tail")
    paged = s3.list_parts(Bucket=name, Key="m", UploadId=uid, MaxParts=1)
    assert [p["PartNumber"] for p in paged["Parts"]] == [1] and paged["IsTruncated"] is True
    snapshot.error("bad-order", lambda: s3.complete_multipart_upload(
        Bucket=name, Key="m", UploadId=uid, MultipartUpload={"Parts": [
            {"PartNumber": 3, "ETag": '"x"'}, {"PartNumber": 1, "ETag": '"y"'}]}), drop=("UploadId",))
    snapshot.error("missing-part", lambda: s3.complete_multipart_upload(
        Bucket=name, Key="m", UploadId=uid, MultipartUpload={"Parts": [{"PartNumber": 2, "ETag": '"z"'}]}), drop=("UploadId",))
    s3.abort_multipart_upload(Bucket=name, Key="m", UploadId=uid)
    snapshot.error("after-abort", lambda: s3.list_parts(Bucket=name, Key="m", UploadId=uid), drop=("UploadId",))
    snapshot.error("part-number-range", lambda: s3.upload_part(Bucket=name, Key="m", UploadId=uid, PartNumber=10001, Body=b"x"),
                   drop=("UploadId",))


def test_conditional_reads_and_writes(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    etag = s3.put_object(Bucket=name, Key="k", Body=b"v1")["ETag"]
    snapshot.error("if-none-match-304", lambda: s3.get_object(Bucket=name, Key="k", IfNoneMatch=etag))
    snapshot.error("if-match-mismatch", lambda: s3.get_object(Bucket=name, Key="k", IfMatch='"nope"'))
    assert s3.get_object(Bucket=name, Key="k", IfMatch=etag)["Body"].read() == b"v1"
    snapshot.error("create-only-if-absent", lambda: s3.put_object(Bucket=name, Key="k", Body=b"v2", IfNoneMatch="*"))
    s3.put_object(Bucket=name, Key="fresh", Body=b"v", IfNoneMatch="*")
    snapshot.error("if-match-write", lambda: s3.put_object(Bucket=name, Key="k", Body=b"v3", IfMatch='"nope"'))


def test_copy_between_buckets_with_versions_and_tags(client, names, cleanup, snapshot):
    s3 = client("s3")
    src, dst = bucket(s3, names, cleanup, "src"), bucket(s3, names, cleanup, "dst")
    s3.put_bucket_versioning(Bucket=src, VersioningConfiguration={"Status": "Enabled"})
    old = s3.put_object(Bucket=src, Key="k", Body=b"old", Tagging="a=1")["VersionId"]
    s3.put_object(Bucket=src, Key="k", Body=b"new")
    out = s3.copy_object(Bucket=dst, Key="from-old", CopySource={"Bucket": src, "Key": "k", "VersionId": old})
    assert out["CopySourceVersionId"] == old
    assert s3.get_object(Bucket=dst, Key="from-old")["Body"].read() == b"old"
    snapshot.match("tags-copied", s3.get_object_tagging(Bucket=dst, Key="from-old"))
    s3.copy_object(Bucket=dst, Key="retagged", CopySource={"Bucket": src, "Key": "k", "VersionId": old},
                   TaggingDirective="REPLACE", Tagging="b=2")
    snapshot.match("tags-replaced", s3.get_object_tagging(Bucket=dst, Key="retagged"))
    snapshot.error("copy-from-absent-bucket", lambda: s3.copy_object(
        Bucket=dst, Key="x", CopySource={"Bucket": names("absent"), "Key": "k"}))


def test_checksums_are_held_to_what_was_sent(client, names, cleanup, snapshot):
    import base64
    import hashlib

    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    body = b"checksummed body"
    sha = base64.b64encode(hashlib.sha256(body).digest()).decode()
    snapshot.match("sha256", s3.put_object(Bucket=name, Key="s", Body=body, ChecksumAlgorithm="SHA256", ChecksumSHA256=sha))
    snapshot.error("wrong-sha256", lambda: s3.put_object(
        Bucket=name, Key="bad", Body=body, ChecksumAlgorithm="SHA256", ChecksumSHA256=base64.b64encode(b"x" * 32).decode()))
    snapshot.error("wrong-md5", lambda: s3.put_object(Bucket=name, Key="bad", Body=body, ContentMD5=base64.b64encode(b"y" * 16).decode()))
    snapshot.error("not-stored", lambda: s3.head_object(Bucket=name, Key="bad"))
    snapshot.match("attributes", s3.get_object_attributes(
        Bucket=name, Key="s", ObjectAttributes=["ETag", "Checksum", "ObjectSize", "StorageClass"]))


def test_lifecycle_rules_are_checked_when_put(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)

    def put(rule):
        return lambda: s3.put_bucket_lifecycle_configuration(Bucket=name, LifecycleConfiguration={"Rules": [rule]})

    snapshot.error("no-action", put({"ID": "a", "Status": "Enabled", "Filter": {"Prefix": ""}}))
    snapshot.error("bad-status", put({"ID": "a", "Status": "Maybe", "Filter": {}, "Expiration": {"Days": 1}}))
    snapshot.error("zero-days", put({"ID": "a", "Status": "Enabled", "Filter": {}, "Expiration": {"Days": 0}}))
    snapshot.match("size-and-tag", s3.put_bucket_lifecycle_configuration(Bucket=name, LifecycleConfiguration={"Rules": [
        {"ID": "a", "Status": "Enabled", "Expiration": {"Days": 30}, "Filter": {"And": {
            "Prefix": "logs/", "Tags": [{"Key": "t", "Value": "1"}], "ObjectSizeGreaterThan": 10}}}]}))
    snapshot.match("kept-as-sent", s3.get_bucket_lifecycle_configuration(Bucket=name))


def test_refused_by_name(client, names, cleanup, snapshot):
    s3 = client("s3")
    name = bucket(s3, names, cleanup)
    snapshot.error("bucket-acl", lambda: s3.get_bucket_acl(Bucket=name))
    s3.put_object(Bucket=name, Key="k", Body=b"x")
    snapshot.error("object-acl", lambda: s3.get_object_acl(Bucket=name, Key="k"))
    snapshot.error("restore", lambda: s3.restore_object(Bucket=name, Key="k", RestoreRequest={"Days": 1}))
