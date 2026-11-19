"""Secrets Manager through boto3.

Cost on AWS: a secret is billed per month, prorated, and each one here lives
for seconds. Cleanup deletes without a recovery window.
"""

# The ARN ends in six random characters on both sides.
ARN = ("ARN",)


def secret(sm, names, cleanup, value="s3cret", label="secret"):
    name = names(label)
    created = sm.create_secret(Name=name, SecretString=value)
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    return name, created


def test_secret_lifecycle_and_stages(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    name, created = secret(sm, names, cleanup)
    snapshot.match("create", created, opaque=ARN)
    snapshot.match("get", sm.get_secret_value(SecretId=name), opaque=ARN)

    snapshot.match("put-new-value", sm.put_secret_value(SecretId=name, SecretString="rotated"),
                   opaque=ARN)
    snapshot.match("get-current", sm.get_secret_value(SecretId=name), opaque=ARN)
    snapshot.match("get-previous", sm.get_secret_value(SecretId=name, VersionStage="AWSPREVIOUS"),
                   opaque=ARN)
    snapshot.match("versions", sm.list_secret_version_ids(SecretId=name),
                   opaque=ARN, unordered=("Versions",))
    snapshot.match("describe", sm.describe_secret(SecretId=name), opaque=ARN)

    snapshot.match("update-description", sm.update_secret(SecretId=name, Description="for tests"),
                   opaque=ARN)
    snapshot.match("tag", sm.tag_resource(SecretId=name, Tags=[{"Key": "env", "Value": "dev"}]))
    snapshot.match("described-again", sm.describe_secret(SecretId=name), opaque=ARN)


def test_binary_secret(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    name = names("secret")
    snapshot.match("create", sm.create_secret(Name=name, SecretBinary=b"\x00\x01\xfe"), opaque=ARN)
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    snapshot.match("get", sm.get_secret_value(SecretId=name), opaque=ARN)


def test_deletion_window_and_restore(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    name, _ = secret(sm, names, cleanup)
    snapshot.match("delete", sm.delete_secret(SecretId=name, RecoveryWindowInDays=7), opaque=ARN)
    snapshot.error("get-while-deleted", lambda: sm.get_secret_value(SecretId=name))
    snapshot.error("put-while-deleted", lambda: sm.put_secret_value(
        SecretId=name, SecretString="x"))
    snapshot.match("describe-while-deleted", sm.describe_secret(SecretId=name), opaque=ARN)
    snapshot.match("restore", sm.restore_secret(SecretId=name), opaque=ARN)
    snapshot.match("get-after-restore", sm.get_secret_value(SecretId=name), opaque=ARN)


def test_refusals(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    absent = names("absent")
    snapshot.error("get-absent", lambda: sm.get_secret_value(SecretId=absent))
    snapshot.error("describe-absent", lambda: sm.describe_secret(SecretId=absent))
    snapshot.error("bad-name", lambda: sm.create_secret(Name="has space!", SecretString="x"))

    name, _ = secret(sm, names, cleanup)
    snapshot.error("create-twice", lambda: sm.create_secret(Name=name, SecretString="other"))
    snapshot.error("string-and-binary", lambda: sm.put_secret_value(
        SecretId=name, SecretString="x", SecretBinary=b"y"))
    snapshot.error("absent-stage", lambda: sm.get_secret_value(
        SecretId=name, VersionStage="NOSUCHSTAGE"))
    snapshot.error("absent-version", lambda: sm.get_secret_value(
        SecretId=name, VersionId="00000000-0000-4000-8000-000000000000"))
    snapshot.error("window-and-force", lambda: sm.delete_secret(
        SecretId=name, RecoveryWindowInDays=7, ForceDeleteWithoutRecovery=True))
    snapshot.error("window-too-short", lambda: sm.delete_secret(
        SecretId=name, RecoveryWindowInDays=3))


# Added 2026-11-12, before they were recorded: UNVERIFIED until recorded against AWS. They cover listing and its filters, batch
# reads, random passwords, resource policies, moving a staging label, client request tokens, and deleting without recovery.


def test_listing_filters_sorting_and_paging(client, names, cleanup, snapshot, eventually):
    sm = client("secretsmanager")
    stem = names("list")
    made = {}
    for leaf, description in (("a", "alpha blue"), ("b", "beta"), ("c", "gamma blue")):
        made[leaf] = f"{stem}-{leaf}"
        sm.create_secret(Name=made[leaf], SecretString="v", Description=description,
                         Tags=[{"Key": "env", "Value": leaf}])
        cleanup(sm.delete_secret, SecretId=made[leaf], ForceDeleteWithoutRecovery=True)
    names_of = lambda r: [s["Name"] for s in r["SecretList"]]  # noqa: E731

    # A listing straight after a create may not show it yet on AWS (two of three, 2026-11-15).
    def all_three():
        listed = names_of(sm.list_secrets(Filters=[{"Key": "name", "Values": [stem]}]))
        assert len(listed) == 3, listed
        return listed

    snapshot.match("by-name", {"names": eventually(all_three)})
    snapshot.match("by-description", {"names": names_of(sm.list_secrets(
        Filters=[{"Key": "name", "Values": [stem]}, {"Key": "description", "Values": ["blue"]}]))})
    snapshot.match("not-name", {"names": names_of(sm.list_secrets(
        Filters=[{"Key": "name", "Values": [stem]}, {"Key": "name", "Values": [f"!{made['a']}"]}]))})
    snapshot.match("by-tag-key", {"names": names_of(sm.list_secrets(
        Filters=[{"Key": "name", "Values": [stem]}, {"Key": "tag-key", "Values": ["env"]}]))})
    snapshot.match("descending", {"names": names_of(sm.list_secrets(
        Filters=[{"Key": "name", "Values": [stem]}], SortOrder="desc"))})
    page = sm.list_secrets(Filters=[{"Key": "name", "Values": [stem]}], MaxResults=2)
    snapshot.match("page", {"count": len(page["SecretList"]), "more": "NextToken" in page})
    snapshot.match("entry", sm.list_secrets(Filters=[{"Key": "name", "Values": [made["a"]]}]),
                   opaque=("ARN",))
    snapshot.error("bad-filter", lambda: sm.list_secrets(Filters=[{"Key": "nonsense", "Values": ["x"]}]))


def test_batch_get_secret_value(client, names, cleanup, snapshot):
    # Kept apart: the recorder's role was refused secretsmanager:BatchGetSecretValue on 2026-11-15, and one
    # refusal should not keep the password and policy labels below from being recorded.
    sm = client("secretsmanager")
    name = names("batch")
    sm.create_secret(Name=name, SecretString="one")
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    snapshot.match("batch", sm.batch_get_secret_value(SecretIdList=[name, f"{name}-absent"]),
                   opaque=("ARN",))


def test_random_password_and_policies(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    name = names("policy")
    sm.create_secret(Name=name, SecretString="one")
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    password = sm.get_random_password(PasswordLength=24, ExcludePunctuation=True)["RandomPassword"]
    snapshot.match("password-shape", {
        "length": len(password), "alnum": password.isalnum(),
        "has-digit": any(c.isdigit() for c in password),
        "has-upper": any(c.isupper() for c in password), "has-lower": any(c.islower() for c in password)})
    snapshot.match("password-default-length", {"length": len(sm.get_random_password()["RandomPassword"])})
    snapshot.error("password-nothing-left", lambda: sm.get_random_password(
        ExcludeUppercase=True, ExcludeLowercase=True, ExcludeNumbers=True, ExcludePunctuation=True))
    policy = ('{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"*"},'
              '"Action":"secretsmanager:GetSecretValue","Resource":"*","Condition":{"StringEquals":'
              '{"aws:PrincipalOrgID":"o-nonexistent"}}}]}')
    snapshot.match("no-policy-yet", sm.get_resource_policy(SecretId=name), opaque=("ARN",))
    snapshot.match("put-policy", sm.put_resource_policy(SecretId=name, ResourcePolicy=policy), opaque=("ARN",))
    snapshot.match("get-policy", sm.get_resource_policy(SecretId=name), opaque=("ARN", "ResourcePolicy"))
    snapshot.match("validate", sm.validate_resource_policy(SecretId=name, ResourcePolicy=policy))
    snapshot.error("put-bad-policy", lambda: sm.put_resource_policy(SecretId=name, ResourcePolicy="not json"))
    snapshot.match("delete-policy", sm.delete_resource_policy(SecretId=name), opaque=("ARN",))


def test_staging_labels_tokens_and_deletion(client, names, cleanup, snapshot, normalizer):
    sm = client("secretsmanager")
    name = names("stages")
    one = sm.create_secret(Name=name, SecretString="1", ClientRequestToken="11111111-2222-4333-8444-555555555555")
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    snapshot.match("create-with-token", one, opaque=("ARN",))
    normalizer.name(one["ARN"], "secret-arn")  # a refusal below quotes it; it ends in six random characters
    snapshot.match("same-token-same-value", sm.put_secret_value(
        SecretId=name, SecretString="1", ClientRequestToken="11111111-2222-4333-8444-555555555555"), opaque=("ARN",))
    snapshot.error("same-token-other-value", lambda: sm.put_secret_value(
        SecretId=name, SecretString="2", ClientRequestToken="11111111-2222-4333-8444-555555555555"))
    two = sm.put_secret_value(SecretId=name, SecretString="2", VersionStages=["PENDING"],
                              ClientRequestToken="22222222-2222-4333-8444-555555555555")
    snapshot.match("put-pending", two, opaque=("ARN",))
    snapshot.match("current-is-still-one", sm.get_secret_value(SecretId=name), opaque=("ARN",))
    snapshot.match("move", sm.update_secret_version_stage(
        SecretId=name, VersionStage="AWSCURRENT", MoveToVersionId=two["VersionId"],
        RemoveFromVersionId=one["VersionId"]), opaque=("ARN",))
    snapshot.match("now-current", sm.get_secret_value(SecretId=name), opaque=("ARN",))
    snapshot.match("previous", sm.get_secret_value(SecretId=name, VersionStage="AWSPREVIOUS"), opaque=("ARN",))
    snapshot.error("drop-current", lambda: sm.update_secret_version_stage(
        SecretId=name, VersionStage="AWSCURRENT", RemoveFromVersionId=two["VersionId"]))
    snapshot.error("neither-version", lambda: sm.update_secret_version_stage(
        SecretId=name, VersionStage="X"))
    snapshot.error("move-to-absent", lambda: sm.update_secret_version_stage(
        SecretId=name, VersionStage="X", MoveToVersionId="00000000-0000-4000-8000-000000000000"))
    snapshot.match("by-version-id", sm.get_secret_value(SecretId=name, VersionId=one["VersionId"]), opaque=("ARN",))
    snapshot.match("tag", sm.tag_resource(SecretId=name, Tags=[{"Key": "a", "Value": "1"}, {"Key": "b", "Value": "2"}]))
    snapshot.match("untag", sm.untag_resource(SecretId=name, TagKeys=["a", "absent"]))
    snapshot.match("tags-now", sm.describe_secret(SecretId=name), opaque=("ARN",))
    snapshot.match("restore-when-not-deleted", sm.restore_secret(SecretId=name), opaque=("ARN",))
    snapshot.match("delete-now", sm.delete_secret(SecretId=name, ForceDeleteWithoutRecovery=True), opaque=("ARN",))
    # AWS deletes without recovery asynchronously: a describe straight after still answered (2026-11-15), and a
    # create under the same name races the deletion. Neither is asserted; the empty secret gets a fresh name.
    empty = names("empty")
    snapshot.match("create-with-no-value", sm.create_secret(Name=empty), opaque=("ARN",))
    cleanup(sm.delete_secret, SecretId=empty, ForceDeleteWithoutRecovery=True)
    snapshot.error("get-with-no-value", lambda: sm.get_secret_value(SecretId=empty))


# Added 2026-11-15 (stream ops1), before it was recorded: UNVERIFIED until recorded against AWS. Replicas in another region.

REPLICA_REGION = "us-west-2"


def in_sync(sm, name, tries=60):
    """AWS replicates in the background: wait until every replica says InSync (doze-aws is in sync when the call answers)."""
    import time
    for _ in range(tries):
        status = sm.describe_secret(SecretId=name).get("ReplicationStatus", [])
        if status and all(s["Status"] != "InProgress" for s in status):
            return status
        time.sleep(1)
    return sm.describe_secret(SecretId=name).get("ReplicationStatus", [])


def test_replica_follows_its_primary(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    west = client("secretsmanager", region_name=REPLICA_REGION)
    name = names("replicated")
    made = sm.create_secret(Name=name, SecretString="one", AddReplicaRegions=[{"Region": REPLICA_REGION}])
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    cleanup(sm.remove_regions_from_replication, SecretId=name, RemoveReplicaRegions=[REPLICA_REGION])
    # The ARN ends in six random characters, and a refusal names it.
    snapshot.known(made["ARN"], "secret-arn")
    snapshot.match("create", made, opaque=("ARN", "VersionId"))
    snapshot.match("in-sync", in_sync(sm, name))
    snapshot.match("describe-primary", sm.describe_secret(SecretId=name),
                   opaque=("ARN", "LastChangedDate", "CreatedDate", "VersionIdsToStages", "LastAccessedDate"))
    assert west.get_secret_value(SecretId=name)["SecretString"] == "one"
    snapshot.match("describe-replica", west.describe_secret(SecretId=name),
                   opaque=("ARN", "LastChangedDate", "CreatedDate", "VersionIdsToStages", "LastAccessedDate"))
    sm.put_secret_value(SecretId=name, SecretString="two")
    in_sync(sm, name)
    assert west.get_secret_value(SecretId=name)["SecretString"] == "two"
    snapshot.error("put-on-replica", lambda: west.put_secret_value(SecretId=name, SecretString="x"))
    snapshot.error("delete-primary-with-replicas", lambda: sm.delete_secret(SecretId=name))
    # Asking again for a region that already holds the replica is not refused (recorded).
    snapshot.outcome("replicate-again", lambda: sm.replicate_secret_to_regions(
        SecretId=name, AddReplicaRegions=[{"Region": REPLICA_REGION}]), opaque=("ARN",))
    # Made again in the background, as at first: it settles back in sync.
    snapshot.match("after-replicate-again", {"replicas": [
        {k: r.get(k) for k in ("Region", "Status", "StatusMessage")} for r in in_sync(sm, name)]})
    snapshot.match("remove", sm.remove_regions_from_replication(
        SecretId=name, RemoveReplicaRegions=[REPLICA_REGION]), opaque=("ARN",))
    snapshot.error("stop-on-a-primary", lambda: sm.stop_replication_to_replica(SecretId=name))


def test_replica_promoted_with_stop_replication(client, names, cleanup, snapshot):
    sm = client("secretsmanager")
    west = client("secretsmanager", region_name=REPLICA_REGION)
    name = names("promoted")
    sm.create_secret(Name=name, SecretString="v")
    cleanup(sm.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    cleanup(west.delete_secret, SecretId=name, ForceDeleteWithoutRecovery=True)
    snapshot.match("replicate", sm.replicate_secret_to_regions(
        SecretId=name, AddReplicaRegions=[{"Region": REPLICA_REGION}]), opaque=("ARN",))
    in_sync(sm, name)
    snapshot.match("stop", west.stop_replication_to_replica(SecretId=name), opaque=("ARN",))
    snapshot.match("put-on-promoted", west.put_secret_value(SecretId=name, SecretString="own"),
                   opaque=("ARN", "VersionId"))
    described = sm.describe_secret(SecretId=name)
    snapshot.match("primary-after", {"replicas": described.get("ReplicationStatus", [])})
