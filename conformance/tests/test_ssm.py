"""SSM Parameter Store through boto3."""


def test_parameter_lifecycle_and_versions(client, names, cleanup, snapshot):
    ssm = client("ssm")
    name = f"/{names('param')}/db/host"
    cleanup(ssm.delete_parameter, Name=name)

    snapshot.match("put", ssm.put_parameter(Name=name, Value="one", Type="String"))
    snapshot.match("get", ssm.get_parameter(Name=name))
    snapshot.error("put-without-overwrite", lambda: ssm.put_parameter(
        Name=name, Value="two", Type="String"))
    snapshot.match("overwrite", ssm.put_parameter(Name=name, Value="two", Overwrite=True))
    snapshot.match("get-v2", ssm.get_parameter(Name=name))
    snapshot.match("get-by-version", ssm.get_parameter(Name=f"{name}:1"))

    snapshot.match("label", ssm.label_parameter_version(
        Name=name, ParameterVersion=1, Labels=["stable"]))
    snapshot.match("get-by-label", ssm.get_parameter(Name=f"{name}:stable"))
    snapshot.match("history", ssm.get_parameter_history(Name=name), opaque=("LastModifiedUser",))
    snapshot.match("describe", ssm.describe_parameters(
        ParameterFilters=[{"Key": "Name", "Values": [name]}]), opaque=("LastModifiedUser",))

    snapshot.match("delete", ssm.delete_parameter(Name=name))
    snapshot.error("get-deleted", lambda: ssm.get_parameter(Name=name))


def test_secure_string_is_ciphertext_until_asked(client, names, cleanup, snapshot):
    ssm = client("ssm")
    name = f"/{names('param')}/password"
    cleanup(ssm.delete_parameter, Name=name)

    snapshot.match("put", ssm.put_parameter(Name=name, Value="hunter2", Type="SecureString"))
    sealed = ssm.get_parameter(Name=name)
    assert sealed["Parameter"]["Value"] != "hunter2", "a SecureString came back in the clear"
    snapshot.match("sealed", sealed, opaque=("Value",))
    snapshot.match("opened", ssm.get_parameter(Name=name, WithDecryption=True))


def test_paths_and_bulk_reads(client, names, cleanup, snapshot):
    ssm = client("ssm")
    root = f"/{names('param')}"
    for leaf in ("a", "b", "nested/c"):
        ssm.put_parameter(Name=f"{root}/{leaf}", Value=leaf, Type="String")
        cleanup(ssm.delete_parameter, Name=f"{root}/{leaf}")

    snapshot.match("one-level", ssm.get_parameters_by_path(Path=root), unordered=("Parameters",))
    snapshot.match("recursive", ssm.get_parameters_by_path(Path=root, Recursive=True),
                   unordered=("Parameters",))
    # Which two come first is not promised; that there are two and more is.
    page = ssm.get_parameters_by_path(Path=root, Recursive=True, MaxResults=2)
    snapshot.match("page", {"count": len(page["Parameters"]), "more": "NextToken" in page})
    snapshot.match("some-missing", ssm.get_parameters(
        Names=[f"{root}/a", f"{root}/absent"]))
    snapshot.match("delete-many", ssm.delete_parameters(
        Names=[f"{root}/a", f"{root}/absent"]))


def test_refusals(client, names, cleanup, snapshot):
    ssm = client("ssm")
    root = f"/{names('param')}"
    snapshot.error("get-absent", lambda: ssm.get_parameter(Name=f"{root}/absent"))
    snapshot.error("delete-absent", lambda: ssm.delete_parameter(Name=f"{root}/absent"))
    # Each refusal has its own name, so one that is wrongly accepted leaves
    # nothing behind for the next to trip over.
    for leaf in ("untyped", "mistyped"):
        cleanup(ssm.delete_parameter, Name=f"{root}/{leaf}")
    snapshot.error("no-type-on-create", lambda: ssm.put_parameter(
        Name=f"{root}/untyped", Value="v"))
    snapshot.error("bad-type", lambda: ssm.put_parameter(
        Name=f"{root}/mistyped", Value="v", Type="Sentence"))
    snapshot.error("reserved-prefix", lambda: ssm.put_parameter(
        Name="/aws/not-yours", Value="v", Type="String"))
    snapshot.error("reserved-word", lambda: ssm.put_parameter(
        Name="ssm-not-yours", Value="v", Type="String"))
    # A path whose first segment merely BEGINS with a reserved word. The
    # documentation does not say; whatever AWS does, do that.
    cleanup(ssm.delete_parameter, Name="/awsome-dzc/x")
    cleanup(ssm.delete_parameter, Name=f"{root}/aws-inside")
    snapshot.outcome("reserved-word-starts-a-segment", lambda: ssm.put_parameter(
        Name="/awsome-dzc/x", Value="v", Type="String"))
    snapshot.outcome("reserved-word-deeper-in-the-path", lambda: ssm.put_parameter(
        Name=f"{root}/aws-inside", Value="v", Type="String"))
    snapshot.error("bad-characters", lambda: ssm.put_parameter(
        Name=f"{root}/has space", Value="v", Type="String"))
    snapshot.error("path-without-slash", lambda: ssm.get_parameters_by_path(Path="no-leading-slash"))

    ssm.put_parameter(Name=f"{root}/x", Value="v", Type="String")
    cleanup(ssm.delete_parameter, Name=f"{root}/x")
    snapshot.error("absent-version", lambda: ssm.get_parameter(Name=f"{root}/x:9"))
    snapshot.error("label-absent-version", lambda: ssm.label_parameter_version(
        Name=f"{root}/x", ParameterVersion=9, Labels=["l"]))
    snapshot.outcome("change-type-on-overwrite", lambda: ssm.put_parameter(
        Name=f"{root}/x", Value="a,b", Type="StringList", Overwrite=True))


# Added 2026-11-12, before they were recorded: they run against doze-aws and are UNVERIFIED until they are recorded against AWS.
# They cover what was built without a scenario: tags, filters, unlabel, SecureString reads in bulk and in history, the value limit
# and AllowedPattern, what an overwrite keeps, and the Secrets Manager reference path.


def test_tags_filters_and_unlabel(client, names, cleanup, snapshot):
    ssm = client("ssm")
    root = f"/{names('param')}"
    a, b = f"{root}/a", f"{root}/b"
    for name in (a, b):
        cleanup(ssm.delete_parameter, Name=name)
    snapshot.match("put-with-tags", ssm.put_parameter(
        Name=a, Value="1", Type="String", Tags=[{"Key": "env", "Value": "dev"}]))
    ssm.put_parameter(Name=b, Value="x,y", Type="StringList")
    snapshot.match("add-tags", ssm.add_tags_to_resource(
        ResourceType="Parameter", ResourceId=a, Tags=[{"Key": "team", "Value": "x"}]))
    snapshot.match("list-tags", ssm.list_tags_for_resource(ResourceType="Parameter", ResourceId=a),
                   unordered=("TagList",))
    snapshot.match("remove-tags", ssm.remove_tags_from_resource(
        ResourceType="Parameter", ResourceId=a, TagKeys=["env"]))
    snapshot.match("tags-after", ssm.list_tags_for_resource(ResourceType="Parameter", ResourceId=a))
    snapshot.error("tags-of-absent", lambda: ssm.list_tags_for_resource(
        ResourceType="Parameter", ResourceId=f"{root}/absent"))
    snapshot.error("tags-of-a-document-type", lambda: ssm.list_tags_for_resource(
        ResourceType="Nonsense", ResourceId=a))
    snapshot.match("by-type", ssm.describe_parameters(ParameterFilters=[
        {"Key": "Type", "Values": ["StringList"]}, {"Key": "Name", "Option": "BeginsWith", "Values": [root]}]),
        opaque=("LastModifiedUser",))
    snapshot.match("by-path-one-level", ssm.describe_parameters(ParameterFilters=[
        {"Key": "Path", "Option": "OneLevel", "Values": [root]}]),
        opaque=("LastModifiedUser",), unordered=("Parameters",))
    snapshot.error("bad-filter-key", lambda: ssm.describe_parameters(ParameterFilters=[
        {"Key": "Nonsense", "Values": ["x"]}]))
    ssm.put_parameter(Name=a, Value="2", Overwrite=True)
    snapshot.match("label-two", ssm.label_parameter_version(Name=a, ParameterVersion=2, Labels=["one", "two"]))
    snapshot.match("unlabel", ssm.unlabel_parameter_version(Name=a, ParameterVersion=2, Labels=["one", "absent"]))
    snapshot.match("label-bad", ssm.label_parameter_version(Name=a, ParameterVersion=2, Labels=["1bad"]))


def test_secure_strings_read_in_bulk_and_in_history(client, names, cleanup, snapshot):
    ssm = client("ssm")
    root = f"/{names('param')}"
    name = f"{root}/secret"
    cleanup(ssm.delete_parameter, Name=name)
    ssm.put_parameter(Name=name, Value="first", Type="SecureString")
    snapshot.match("overwrite-keeps-type", ssm.put_parameter(Name=name, Value="second", Overwrite=True))
    snapshot.match("history-open", ssm.get_parameter_history(Name=name, WithDecryption=True),
                   opaque=("LastModifiedUser",))
    history = ssm.get_parameter_history(Name=name)
    snapshot.match("history-sealed", history, opaque=("LastModifiedUser", "Value"))
    snapshot.match("bulk-open", ssm.get_parameters(Names=[name], WithDecryption=True))
    snapshot.match("by-path-open", ssm.get_parameters_by_path(Path=root, WithDecryption=True))
    snapshot.match("describe", ssm.describe_parameters(ParameterFilters=[
        {"Key": "Name", "Values": [name]}]), opaque=("LastModifiedUser",))


def test_value_limits_and_allowed_pattern(client, names, cleanup, snapshot):
    ssm = client("ssm")
    root = f"/{names('param')}"
    for leaf in ("big", "digits"):
        cleanup(ssm.delete_parameter, Name=f"{root}/{leaf}")
    snapshot.error("over-4096-standard", lambda: ssm.put_parameter(
        Name=f"{root}/big", Value="x" * 4097, Type="String"))
    snapshot.match("exactly-4096", ssm.put_parameter(Name=f"{root}/big", Value="x" * 4096, Type="String",
                                                     Overwrite=True))
    snapshot.match("pattern", ssm.put_parameter(Name=f"{root}/digits", Value="42", Type="String",
                                                AllowedPattern=r"^\d+$"))
    snapshot.error("pattern-mismatch", lambda: ssm.put_parameter(
        Name=f"{root}/digits", Value="forty", Type="String", Overwrite=True))
    snapshot.match("pattern-kept", ssm.describe_parameters(ParameterFilters=[
        {"Key": "Name", "Values": [f"{root}/digits"]}]), opaque=("LastModifiedUser",))
    snapshot.error("bad-pattern", lambda: ssm.put_parameter(
        Name=f"{root}/digits", Value="1", Type="String", AllowedPattern="(", Overwrite=True))
    snapshot.error("tags-with-overwrite", lambda: ssm.put_parameter(
        Name=f"{root}/digits", Value="1", Type="String", Overwrite=True, Tags=[{"Key": "k", "Value": "v"}]))
    snapshot.error("bad-data-type", lambda: ssm.put_parameter(
        Name=f"{root}/digits", Value="1", Type="String", DataType="Text", Overwrite=True))
    snapshot.error("policies-on-standard", lambda: ssm.put_parameter(
        Name=f"{root}/digits", Value="1", Type="String", Overwrite=True,
        Policies='[{"Type":"Expiration","Version":"1.0","Attributes":{"Timestamp":"2099-01-01T00:00:00.000Z"}}]'))
    snapshot.outcome("description-after-overwrite", lambda: (
        ssm.put_parameter(Name=f"{root}/digits", Value="7", Type="String", Overwrite=True, Description="kept?"),
        ssm.put_parameter(Name=f"{root}/digits", Value="8", Type="String", Overwrite=True),
        ssm.describe_parameters(ParameterFilters=[{"Key": "Name", "Values": [f"{root}/digits"]}])))


def test_a_secrets_manager_secret_read_through_parameter_store(client, names, cleanup, snapshot):
    ssm = client("ssm")
    sm = client("secretsmanager")
    secret = names("secret")
    sm.create_secret(Name=secret, SecretString="s3cret")
    cleanup(sm.delete_secret, SecretId=secret, ForceDeleteWithoutRecovery=True)
    reference = f"/aws/reference/secretsmanager/{secret}"
    snapshot.match("open", ssm.get_parameter(Name=reference, WithDecryption=True), opaque=("Version",))
    snapshot.error("sealed", lambda: ssm.get_parameter(Name=reference))
    snapshot.match("bulk", ssm.get_parameters(
        Names=[reference, f"{reference}-absent"], WithDecryption=True), opaque=("Version",))
    snapshot.error("absent", lambda: ssm.get_parameter(
        Name=f"{reference}-absent", WithDecryption=True))
    snapshot.outcome("by-path-does-not-list-it", lambda: ssm.get_parameters_by_path(
        Path="/aws/reference/secretsmanager", WithDecryption=True))
