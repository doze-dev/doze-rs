"""KMS through boto3.

Cost on AWS: a key is $1 a month, prorated by the hour, and stops being billed
once it is scheduled for deletion — which every key here is, at the 7-day
minimum, when its test ends. A full run is a fraction of a cent, and leaves
keys in PendingDeletion for a week.
"""

# Ciphertext and generated key material: random by design.
SECRET = ("CiphertextBlob", "Plaintext", "Signature", "Mac")


def key(kms, cleanup, **kw):
    made = kms.create_key(**kw)
    cleanup(kms.schedule_key_deletion, KeyId=made["KeyMetadata"]["KeyId"], PendingWindowInDays=7)
    return made["KeyMetadata"]["KeyId"], made


def test_symmetric_key_encrypts_and_decrypts(client, cleanup, snapshot):
    kms = client("kms")
    kid, made = key(kms, cleanup, Description="conformance")
    snapshot.match("create", made)
    snapshot.match("describe", kms.describe_key(KeyId=kid))

    sealed = snapshot.match("encrypt", kms.encrypt(KeyId=kid, Plaintext=b"attack at dawn"),
                            opaque=SECRET)
    snapshot.match("decrypt", kms.decrypt(CiphertextBlob=sealed["CiphertextBlob"]))

    bound = kms.encrypt(KeyId=kid, Plaintext=b"x", EncryptionContext={"tenant": "a"})
    snapshot.match("decrypt-with-context", kms.decrypt(
        CiphertextBlob=bound["CiphertextBlob"], EncryptionContext={"tenant": "a"}))
    snapshot.error("decrypt-without-context", lambda: kms.decrypt(
        CiphertextBlob=bound["CiphertextBlob"]))
    snapshot.error("decrypt-wrong-context", lambda: kms.decrypt(
        CiphertextBlob=bound["CiphertextBlob"], EncryptionContext={"tenant": "b"}))

    dk = kms.generate_data_key(KeyId=kid, KeySpec="AES_256")
    assert len(dk["Plaintext"]) == 32
    snapshot.match("data-key", dk, opaque=SECRET)
    snapshot.match("data-key-opens", kms.decrypt(CiphertextBlob=dk["CiphertextBlob"]),
                   opaque=SECRET)
    snapshot.match("rotation-off", kms.get_key_rotation_status(KeyId=kid))
    snapshot.match("enable-rotation", kms.enable_key_rotation(KeyId=kid))
    snapshot.match("rotation-on", kms.get_key_rotation_status(KeyId=kid))


def test_aliases(client, names, cleanup, snapshot):
    kms = client("kms")
    kid, _ = key(kms, cleanup)
    alias = f"alias/{names('alias')}"
    snapshot.match("create", kms.create_alias(AliasName=alias, TargetKeyId=kid))
    cleanup(kms.delete_alias, AliasName=alias)

    snapshot.match("describe-through-alias", kms.describe_key(KeyId=alias))
    sealed = kms.encrypt(KeyId=alias, Plaintext=b"x")
    snapshot.match("encrypt-through-alias", sealed, opaque=SECRET)
    snapshot.match("list-for-key", kms.list_aliases(KeyId=kid))
    snapshot.error("create-twice", lambda: kms.create_alias(AliasName=alias, TargetKeyId=kid))
    snapshot.error("no-alias-prefix", lambda: kms.create_alias(
        AliasName=names("alias"), TargetKeyId=kid))
    snapshot.error("reserved-prefix", lambda: kms.create_alias(
        AliasName="alias/aws/mine", TargetKeyId=kid))
    snapshot.match("delete", kms.delete_alias(AliasName=alias))
    snapshot.error("delete-again", lambda: kms.delete_alias(AliasName=alias))


def test_key_states(client, cleanup, snapshot):
    kms = client("kms")
    kid, _ = key(kms, cleanup)
    snapshot.match("disable", kms.disable_key(KeyId=kid))
    snapshot.match("disabled", kms.describe_key(KeyId=kid))
    snapshot.error("encrypt-while-disabled", lambda: kms.encrypt(KeyId=kid, Plaintext=b"x"))
    snapshot.match("enable", kms.enable_key(KeyId=kid))

    snapshot.match("schedule", kms.schedule_key_deletion(KeyId=kid, PendingWindowInDays=7))
    snapshot.match("pending", kms.describe_key(KeyId=kid))
    snapshot.error("encrypt-while-pending", lambda: kms.encrypt(KeyId=kid, Plaintext=b"x"))
    snapshot.match("cancel", kms.cancel_key_deletion(KeyId=kid))
    snapshot.match("after-cancel", kms.describe_key(KeyId=kid))


def test_asymmetric_sign_and_hmac(client, cleanup, snapshot):
    kms = client("kms")
    signer, made = key(kms, cleanup, KeySpec="ECC_NIST_P256", KeyUsage="SIGN_VERIFY")
    snapshot.match("create-signer", made)
    sig = snapshot.match("sign", kms.sign(
        KeyId=signer, Message=b"msg", SigningAlgorithm="ECDSA_SHA_256"), opaque=SECRET)
    snapshot.match("verify", kms.verify(
        KeyId=signer, Message=b"msg", Signature=sig["Signature"], SigningAlgorithm="ECDSA_SHA_256"))
    snapshot.error("verify-tampered", lambda: kms.verify(
        KeyId=signer, Message=b"other", Signature=sig["Signature"],
        SigningAlgorithm="ECDSA_SHA_256"))
    snapshot.error("encrypt-with-a-signing-key", lambda: kms.encrypt(KeyId=signer, Plaintext=b"x"))
    snapshot.match("public-key", kms.get_public_key(KeyId=signer), opaque=("PublicKey",))

    mac_key, _ = key(kms, cleanup, KeySpec="HMAC_256", KeyUsage="GENERATE_VERIFY_MAC")
    mac = snapshot.match("mac", kms.generate_mac(
        KeyId=mac_key, Message=b"msg", MacAlgorithm="HMAC_SHA_256"), opaque=SECRET)
    snapshot.match("verify-mac", kms.verify_mac(
        KeyId=mac_key, Message=b"msg", Mac=mac["Mac"], MacAlgorithm="HMAC_SHA_256"))


def test_refusals(client, cleanup, snapshot):
    kms = client("kms")
    absent = "00000000-0000-4000-8000-000000000000"
    snapshot.error("describe-absent", lambda: kms.describe_key(KeyId=absent))
    snapshot.error("encrypt-absent", lambda: kms.encrypt(KeyId=absent, Plaintext=b"x"))
    snapshot.error("decrypt-garbage", lambda: kms.decrypt(CiphertextBlob=b"not ciphertext"))
    snapshot.error("absent-alias", lambda: kms.describe_key(KeyId="alias/dzc-never-made"))

    kid, _ = key(kms, cleanup)
    snapshot.error("window-too-short", lambda: kms.schedule_key_deletion(
        KeyId=kid, PendingWindowInDays=3))
    snapshot.error("sign-with-an-encryption-key", lambda: kms.sign(
        KeyId=kid, Message=b"m", SigningAlgorithm="ECDSA_SHA_256"))
    snapshot.error("bad-spec-for-usage", lambda: kms.create_key(
        KeySpec="HMAC_256", KeyUsage="ENCRYPT_DECRYPT"))


# Added 2026-11-12, before they were recorded: UNVERIFIED until recorded against AWS. They cover what was built without a scenario:
# RSA keys, re-encryption, data keys without plaintext and data key pairs, random bytes, key policy, tags, rotation on demand,
# grants, aliases that move, and the refusals around states. (Key import is not here: it needs RSA wrapping in Python.)


def test_rsa_signing_and_encryption(client, cleanup, snapshot):
    kms = client("kms")
    signer, made = key(kms, cleanup, KeySpec="RSA_2048", KeyUsage="SIGN_VERIFY")
    snapshot.match("create-signer", made)
    for algorithm in ("RSASSA_PSS_SHA_256", "RSASSA_PKCS1_V1_5_SHA_384"):
        sig = kms.sign(KeyId=signer, Message=b"msg", SigningAlgorithm=algorithm)
        snapshot.match(f"sign-{algorithm}", {"len": len(sig["Signature"]), "alg": sig["SigningAlgorithm"]})
        snapshot.match(f"verify-{algorithm}", kms.verify(
            KeyId=signer, Message=b"msg", Signature=sig["Signature"], SigningAlgorithm=algorithm))
        snapshot.error(f"tampered-{algorithm}", lambda: kms.verify(
            KeyId=signer, Message=b"other", Signature=sig["Signature"], SigningAlgorithm=algorithm))
    snapshot.error("wrong-algorithm", lambda: kms.sign(
        KeyId=signer, Message=b"m", SigningAlgorithm="ECDSA_SHA_256"))
    snapshot.match("public-key", kms.get_public_key(KeyId=signer), opaque=("PublicKey",))
    snapshot.match("digest-sign", {"len": len(kms.sign(
        KeyId=signer, Message=bytes(32), MessageType="DIGEST", SigningAlgorithm="RSASSA_PSS_SHA_256")["Signature"])})
    snapshot.error("digest-wrong-length", lambda: kms.sign(
        KeyId=signer, Message=b"short", MessageType="DIGEST", SigningAlgorithm="RSASSA_PSS_SHA_256"))

    vault, made = key(kms, cleanup, KeySpec="RSA_2048", KeyUsage="ENCRYPT_DECRYPT")
    snapshot.match("create-vault", made)
    sealed = kms.encrypt(KeyId=vault, Plaintext=b"attack at dawn", EncryptionAlgorithm="RSAES_OAEP_SHA_256")
    snapshot.match("encrypt", sealed, opaque=("CiphertextBlob",))
    snapshot.match("decrypt", kms.decrypt(
        CiphertextBlob=sealed["CiphertextBlob"], KeyId=vault, EncryptionAlgorithm="RSAES_OAEP_SHA_256"))
    snapshot.error("encrypt-without-algorithm", lambda: kms.encrypt(KeyId=vault, Plaintext=b"x"))
    snapshot.error("decrypt-without-key", lambda: kms.decrypt(
        CiphertextBlob=sealed["CiphertextBlob"], EncryptionAlgorithm="RSAES_OAEP_SHA_256"))
    snapshot.error("too-long", lambda: kms.encrypt(
        KeyId=vault, Plaintext=b"x" * 300, EncryptionAlgorithm="RSAES_OAEP_SHA_256"))


def test_re_encrypt_data_keys_and_random(client, cleanup, snapshot):
    kms = client("kms")
    a, _ = key(kms, cleanup)
    b, _ = key(kms, cleanup)
    sealed = kms.encrypt(KeyId=a, Plaintext=b"move me", EncryptionContext={"k": "v"})
    moved = kms.re_encrypt(CiphertextBlob=sealed["CiphertextBlob"], SourceEncryptionContext={"k": "v"},
                           DestinationKeyId=b, DestinationEncryptionContext={"k2": "v2"})
    # Key material ids are a hash of random material: present and consistent is all there is to compare.
    snapshot.match("re-encrypt", moved, opaque=SECRET + ("SourceKeyMaterialId", "DestinationKeyMaterialId"))
    snapshot.match("opens-under-b", kms.decrypt(
        CiphertextBlob=moved["CiphertextBlob"], EncryptionContext={"k2": "v2"}), opaque=SECRET)
    snapshot.error("re-encrypt-wrong-context", lambda: kms.re_encrypt(
        CiphertextBlob=sealed["CiphertextBlob"], DestinationKeyId=b))
    sealed_only = kms.generate_data_key_without_plaintext(KeyId=a, KeySpec="AES_128")
    snapshot.match("without-plaintext", sealed_only, opaque=SECRET)
    snapshot.match("sealed-opens", {"len": len(kms.decrypt(CiphertextBlob=sealed_only["CiphertextBlob"])["Plaintext"])})
    snapshot.match("by-bytes", {"len": len(kms.generate_data_key(KeyId=a, NumberOfBytes=40)["Plaintext"])})
    snapshot.error("both-spec-and-bytes", lambda: kms.generate_data_key(KeyId=a, KeySpec="AES_256", NumberOfBytes=16))
    snapshot.error("neither", lambda: kms.generate_data_key(KeyId=a))
    snapshot.match("random", {"len": len(kms.generate_random(NumberOfBytes=64)["Plaintext"])})
    snapshot.error("random-too-many", lambda: kms.generate_random(NumberOfBytes=2000))
    pair = kms.generate_data_key_pair(KeyId=a, KeyPairSpec="ECC_NIST_P256")
    snapshot.match("pair", pair, opaque=SECRET + ("PrivateKeyCiphertextBlob", "PrivateKeyPlaintext", "PublicKey"))
    snapshot.match("pair-without-plaintext", kms.generate_data_key_pair_without_plaintext(
        KeyId=a, KeyPairSpec="RSA_2048"), opaque=("PrivateKeyCiphertextBlob", "PublicKey", "KeyMaterialId"))
    snapshot.match("pair-private-key-opens", {"same": kms.decrypt(
        CiphertextBlob=pair["PrivateKeyCiphertextBlob"])["Plaintext"] == pair["PrivateKeyPlaintext"]})


def test_policy_tags_rotation_and_grants(client, cleanup, snapshot, target, eventually, normalizer):
    kms = client("kms")
    kid, _ = key(kms, cleanup)
    policy = kms.get_key_policy(KeyId=kid, PolicyName="default")
    snapshot.match("default-policy-shape", {"has-root": ":root" in policy["Policy"], "name": policy["PolicyName"]})
    snapshot.match("policy-names", kms.list_key_policies(KeyId=kid))
    snapshot.error("policy-bad-name", lambda: kms.get_key_policy(KeyId=kid, PolicyName="other"))
    snapshot.error("put-bad-policy", lambda: kms.put_key_policy(KeyId=kid, PolicyName="default", Policy="nope"))
    snapshot.match("tag", kms.tag_resource(KeyId=kid, Tags=[{"TagKey": "a", "TagValue": "1"}]))
    snapshot.match("tags", kms.list_resource_tags(KeyId=kid), unordered=("Tags",))
    snapshot.match("untag", kms.untag_resource(KeyId=kid, TagKeys=["a"]))
    snapshot.error("reserved-tag", lambda: kms.tag_resource(KeyId=kid, Tags=[{"TagKey": "aws:x", "TagValue": "1"}]))
    snapshot.match("describe-key", kms.update_key_description(KeyId=kid, Description="renamed"))
    snapshot.match("described", kms.describe_key(KeyId=kid))
    before = kms.describe_key(KeyId=kid)["KeyMetadata"]["CurrentKeyMaterialId"]
    snapshot.match("enable-rotation-90", kms.enable_key_rotation(KeyId=kid, RotationPeriodInDays=90))
    snapshot.match("rotation-status", kms.get_key_rotation_status(KeyId=kid))
    snapshot.match("rotate-now", kms.rotate_key_on_demand(KeyId=kid))

    # AWS rotates on demand in the background: straight after the call the material had not changed and no rotation was
    # listed (2026-11-15), and the rotation was listed before DescribeKey showed the new material. What is asserted is where
    # it ends up.
    def rotated():
        r = kms.list_key_rotations(KeyId=kid)
        assert r.get("Rotations"), "not rotated yet"
        assert kms.describe_key(KeyId=kid)["KeyMetadata"]["CurrentKeyMaterialId"] != before, "material not changed yet"
        return r

    rotations = eventually(rotated, timeout=300, every=5)
    snapshot.match("material-changed", {"changed": True})
    snapshot.match("rotations", rotations, opaque=("KeyMaterialId",))
    snapshot.match("disable-rotation", kms.disable_key_rotation(KeyId=kid))
    grantee = f"arn:aws:iam::{target.account}:root"
    grant = kms.create_grant(KeyId=kid, GranteePrincipal=grantee, RetiringPrincipal=grantee,
                             Operations=["Decrypt", "Encrypt"], Name="g1")
    snapshot.match("create-grant", grant, opaque=("GrantToken", "GrantId"))
    normalizer.name(grant["GrantId"], "grant-id")  # a refusal below quotes it back
    snapshot.match("same-name-same-grant", {"same": kms.create_grant(
        KeyId=kid, GranteePrincipal=grantee, Operations=["Decrypt"], Name="g1")["GrantId"] == grant["GrantId"]})
    # The order of a listing of grants changed between two recordings: not promised.
    snapshot.match("grants", kms.list_grants(KeyId=kid), opaque=("GrantId", "KeyId", "IssuingAccount"),
                   unordered=("Grants",))
    snapshot.error("bad-operation", lambda: kms.create_grant(
        KeyId=kid, GranteePrincipal=grantee, Operations=["Nonsense"]))
    snapshot.match("retire", kms.retire_grant(GrantToken=grant["GrantToken"]))
    snapshot.match("no-grants", kms.list_grants(KeyId=kid), opaque=("GrantId", "KeyId", "IssuingAccount"))
    snapshot.error("revoke-absent", lambda: kms.revoke_grant(KeyId=kid, GrantId=grant["GrantId"]))


def test_aliases_that_move_and_states(client, names, cleanup, snapshot):
    kms = client("kms")
    one, _ = key(kms, cleanup)
    two, _ = key(kms, cleanup)
    alias = f"alias/{names('alias')}"
    kms.create_alias(AliasName=alias, TargetKeyId=one)
    cleanup(kms.delete_alias, AliasName=alias)
    snapshot.match("update-alias", kms.update_alias(AliasName=alias, TargetKeyId=two))
    snapshot.match("moved", kms.list_aliases(KeyId=two))
    snapshot.match("old-key-has-none", kms.list_aliases(KeyId=one))
    snapshot.error("update-absent", lambda: kms.update_alias(AliasName="alias/dzc-never", TargetKeyId=one))
    snapshot.match("list-keys-has-them", {"both": {one, two} <= {k["KeyId"] for k in kms.list_keys()["Keys"]}})
    snapshot.match("schedule", kms.schedule_key_deletion(KeyId=one, PendingWindowInDays=7))
    snapshot.error("schedule-twice", lambda: kms.schedule_key_deletion(KeyId=one, PendingWindowInDays=7))
    snapshot.error("enable-while-pending", lambda: kms.enable_key(KeyId=one))
    snapshot.error("disable-while-pending", lambda: kms.disable_key(KeyId=one))
    snapshot.error("alias-to-pending", lambda: kms.create_alias(AliasName=f"{alias}-2", TargetKeyId=one))
    snapshot.error("describe-description-while-pending", lambda: kms.update_key_description(KeyId=one, Description="x"))
    snapshot.error("cancel-when-not-pending", lambda: kms.cancel_key_deletion(KeyId=two))
    snapshot.error("rotate-a-signing-key", lambda: kms.enable_key_rotation(
        KeyId=key(kms, cleanup, KeySpec="ECC_NIST_P256", KeyUsage="SIGN_VERIFY")[0]))
    snapshot.error("signing-key-without-usage", lambda: kms.create_key(KeySpec="ECC_NIST_P256"))


# Added 2026-11-15 (stream ops1), before they were recorded: UNVERIFIED until recorded against AWS. Key agreement, multi-Region
# keys, and the last use of a key.

REPLICA_REGION = "us-west-2"


def test_derive_shared_secret(client, cleanup, snapshot):
    kms = client("kms")
    ours, made = key(kms, cleanup, KeySpec="ECC_NIST_P256", KeyUsage="KEY_AGREEMENT")
    theirs, _ = key(kms, cleanup, KeySpec="ECC_NIST_P256", KeyUsage="KEY_AGREEMENT")
    snapshot.match("create", made)
    our_public = kms.get_public_key(KeyId=ours)
    snapshot.match("public-key", our_public, opaque=("PublicKey",))
    their_public = kms.get_public_key(KeyId=theirs)["PublicKey"]
    a = kms.derive_shared_secret(KeyId=ours, KeyAgreementAlgorithm="ECDH", PublicKey=their_public)
    b = kms.derive_shared_secret(KeyId=theirs, KeyAgreementAlgorithm="ECDH", PublicKey=our_public["PublicKey"])
    assert a["SharedSecret"] == b["SharedSecret"], "the two sides derived different secrets"
    snapshot.match("derive", a, opaque=("SharedSecret",))
    other, _ = key(kms, cleanup, KeySpec="ECC_NIST_P384", KeyUsage="KEY_AGREEMENT")
    p384 = kms.get_public_key(KeyId=other)["PublicKey"]
    snapshot.error("other-curve", lambda: kms.derive_shared_secret(
        KeyId=ours, KeyAgreementAlgorithm="ECDH", PublicKey=p384))
    signer, _ = key(kms, cleanup, KeySpec="ECC_NIST_P256", KeyUsage="SIGN_VERIFY")
    snapshot.error("sign-key", lambda: kms.derive_shared_secret(
        KeyId=signer, KeyAgreementAlgorithm="ECDH", PublicKey=their_public))
    snapshot.error("dry-run", lambda: kms.derive_shared_secret(
        KeyId=ours, KeyAgreementAlgorithm="ECDH", PublicKey=their_public, DryRun=True))


def enabled(kms, kid, tries=60):
    """AWS makes a replica in the background (Creating, then Enabled): wait for it."""
    import time
    for _ in range(tries):
        meta = kms.describe_key(KeyId=kid)["KeyMetadata"]
        if meta["KeyState"] != "Creating":
            return meta
        time.sleep(1)
    return kms.describe_key(KeyId=kid)["KeyMetadata"]


def test_multi_region_replica(client, cleanup, snapshot):
    kms = client("kms")
    west = client("kms", region_name=REPLICA_REGION)
    kid, made = key(kms, cleanup, MultiRegion=True, Description="primary")
    assert kid.startswith("mrk-")
    snapshot.match("create", made)
    replica = kms.replicate_key(KeyId=kid, ReplicaRegion=REPLICA_REGION, Description="replica")
    cleanup(west.schedule_key_deletion, KeyId=kid, PendingWindowInDays=7)
    snapshot.match("replicate", replica, opaque=("ReplicaPolicy",))
    snapshot.match("replica-enabled", {"state": enabled(west, kid)["KeyState"]})
    snapshot.match("primary-describe", kms.describe_key(KeyId=kid))
    sealed = kms.encrypt(KeyId=kid, Plaintext=b"across regions")["CiphertextBlob"]
    snapshot.match("replica-decrypts", west.decrypt(CiphertextBlob=sealed), opaque=SECRET)
    snapshot.error("replicate-again", lambda: kms.replicate_key(KeyId=kid, ReplicaRegion=REPLICA_REGION))
    snapshot.error("replicate-a-replica", lambda: west.replicate_key(KeyId=kid, ReplicaRegion="eu-west-1"))
    snapshot.error("rotate-a-replica", lambda: west.rotate_key_on_demand(KeyId=kid))
    single, _ = key(kms, cleanup)
    snapshot.error("replicate-single-region", lambda: kms.replicate_key(KeyId=single, ReplicaRegion=REPLICA_REGION))


def test_update_primary_region(client, cleanup, snapshot):
    kms = client("kms")
    west = client("kms", region_name=REPLICA_REGION)
    kid, _ = key(kms, cleanup, MultiRegion=True)
    kms.replicate_key(KeyId=kid, ReplicaRegion=REPLICA_REGION)
    cleanup(west.schedule_key_deletion, KeyId=kid, PendingWindowInDays=7)
    enabled(west, kid)
    snapshot.error("to-a-region-without-replica", lambda: kms.update_primary_region(
        KeyId=kid, PrimaryRegion="eu-west-1"))
    snapshot.match("update", kms.update_primary_region(KeyId=kid, PrimaryRegion=REPLICA_REGION))
    import time
    # Wait for both sides to settle: a key still Updating refuses ScheduleKeyDeletion, and the cleanup would leave it behind.
    for _ in range(60):
        config = west.describe_key(KeyId=kid)["KeyMetadata"]["MultiRegionConfiguration"]
        if config["MultiRegionKeyType"] == "PRIMARY" and kms.describe_key(KeyId=kid)["KeyMetadata"]["KeyState"] == "Enabled":
            break
        time.sleep(1)
    snapshot.match("west-is-primary", west.describe_key(KeyId=kid)["KeyMetadata"]["MultiRegionConfiguration"])


def test_key_last_usage(client, cleanup, snapshot):
    kms = client("kms")
    kid, _ = key(kms, cleanup)
    snapshot.match("unused", kms.get_key_last_usage(KeyId=kid),
                   opaque=("TrackingStartDate", "KeyCreationDate"))
    kms.encrypt(KeyId=kid, Plaintext=b"x")
    used = kms.get_key_last_usage(KeyId=kid)
    snapshot.match("after-encrypt", used,
                   opaque=("TrackingStartDate", "KeyCreationDate", "Timestamp", "KmsRequestId", "CloudTrailEventId"))
