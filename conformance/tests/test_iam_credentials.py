"""IAM through boto3: what a user holds beside its access keys (MFA devices, SSH keys, signing certificates, service-specific
credentials), server certificates, the credential report and last-accessed data. What Terraform's and CDK's user deletes walk.

The certificates and keys are the crate's test data (made with openssl for these tests, valid until 2126). Nothing here bills.
ChangePassword and SetSecurityTokenServicePreferences change what the recorder may not touch (a console password, an account-wide
setting): those two scenarios run against doze-aws only."""

import base64
import hashlib
import hmac
import json
import pathlib
import struct
import time

import pytest

TESTDATA = pathlib.Path(__file__).resolve().parents[2] / "crates" / "aws-iam" / "src" / "tests" / "testdata"
CERT = (TESTDATA / "rsa-cert.pem").read_text()
KEY = (TESTDATA / "rsa-key.pem").read_text()
OTHER_KEY = (TESTDATA / "other-key.pem").read_text()
OTHER_CERT = (TESTDATA / "other-cert.pem").read_text()
SSH_RSA = (TESTDATA / "rsa.pub").read_text()
SSH_ED25519 = (TESTDATA / "ed25519.pub").read_text()

# IAM's own ids, unique and not ours to predict.
IDS = ("UserId", "CertificateId", "SSHPublicKeyId", "ServerCertificateId", "ServiceSpecificCredentialId",
       "ServicePassword", "AccessKeyId", "SecretAccessKey")


def totp(seed_b32, step):
    """RFC 6238: the six-digit code of a base32 seed at a 30-second step."""
    key = base64.b32decode(seed_b32 + "=" * (-len(seed_b32) % 8))
    digest = hmac.new(key, struct.pack(">Q", step), hashlib.sha1).digest()
    offset = digest[-1] & 0x0F
    return "%06d" % ((struct.unpack(">I", digest[offset:offset + 4])[0] & 0x7FFFFFFF) % 1_000_000)


def two_codes(seed_b32):
    """The previous step's code and this step's: two consecutive codes, both already shown by an app."""
    step = int(time.time()) // 30
    return totp(seed_b32, step - 1), totp(seed_b32, step)


def device(iam, names, cleanup, label="mfa"):
    made = iam.create_virtual_mfa_device(VirtualMFADeviceName=names(label))["VirtualMFADevice"]
    cleanup(iam.delete_virtual_mfa_device, SerialNumber=made["SerialNumber"])
    return made["SerialNumber"], made["Base32StringSeed"].decode()


def test_virtual_mfa_device_lifecycle(client, names, cleanup, snapshot):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    made = iam.create_virtual_mfa_device(
        VirtualMFADeviceName=names("mfa"), Tags=[{"Key": "team", "Value": "a"}])["VirtualMFADevice"]
    serial = made["SerialNumber"]
    cleanup(iam.delete_virtual_mfa_device, SerialNumber=serial)
    # The seed is base32 (RFC 4648) of 20 bytes; the QR code is not compared (doze-aws draws none).
    seed = made["Base32StringSeed"].decode()
    assert len(base64.b32decode(seed + "=" * (-len(seed) % 8))) == 20
    snapshot.match("created", {"SerialNumber": serial, "Tags": made.get("Tags")})
    snapshot.match("unassigned", iam.list_virtual_mfa_devices(AssignmentStatus="Unassigned"),
                   unordered=("VirtualMFADevices",))

    a, b = two_codes(seed)
    snapshot.match("enable", iam.enable_mfa_device(
        UserName=user, SerialNumber=serial, AuthenticationCode1=a, AuthenticationCode2=b))
    cleanup(iam.deactivate_mfa_device, UserName=user, SerialNumber=serial)
    snapshot.match("listed", iam.list_mfa_devices(UserName=user))
    snapshot.match("assigned", iam.list_virtual_mfa_devices(AssignmentStatus="Assigned"), opaque=IDS,
                   unordered=("VirtualMFADevices",))
    snapshot.match("tags", iam.list_mfa_device_tags(SerialNumber=serial))
    snapshot.error("delete-user-with-device", lambda: iam.delete_user(UserName=user))
    snapshot.error("delete-assigned-device", lambda: iam.delete_virtual_mfa_device(SerialNumber=serial))
    snapshot.outcome("get-mfa-device", lambda: iam.get_mfa_device(SerialNumber=serial, UserName=user))
    snapshot.match("deactivate", iam.deactivate_mfa_device(UserName=user, SerialNumber=serial))
    snapshot.match("delete", iam.delete_virtual_mfa_device(SerialNumber=serial))


def test_mfa_refusals(client, names, cleanup, snapshot, target):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    serial, seed = device(iam, names, cleanup)
    snapshot.error("create-twice", lambda: iam.create_virtual_mfa_device(VirtualMFADeviceName=names("mfa")))
    a, b = two_codes(seed)
    snapshot.error("codes-out-of-order", lambda: iam.enable_mfa_device(
        UserName=user, SerialNumber=serial, AuthenticationCode1=b, AuthenticationCode2=a))
    snapshot.error("absent-device", lambda: iam.enable_mfa_device(
        UserName=user, SerialNumber=f"arn:aws:iam::{target.account}:mfa/{names('absent')}",
        AuthenticationCode1=a, AuthenticationCode2=b))
    snapshot.error("deactivate-unassigned", lambda: iam.deactivate_mfa_device(UserName=user, SerialNumber=serial))


def test_session_token_with_an_mfa_code(client, names, cleanup, snapshot, eventually):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    serial, seed = device(iam, names, cleanup)
    a, b = two_codes(seed)
    iam.enable_mfa_device(UserName=user, SerialNumber=serial, AuthenticationCode1=a, AuthenticationCode2=b)
    cleanup(iam.deactivate_mfa_device, UserName=user, SerialNumber=serial)
    key = iam.create_access_key(UserName=user)["AccessKey"]
    cleanup(iam.delete_access_key, UserName=user, AccessKeyId=key["AccessKeyId"])
    sts = client("sts", aws_access_key_id=key["AccessKeyId"], aws_secret_access_key=key["SecretAccessKey"])
    # A new key takes a few seconds to work on AWS.
    eventually(lambda: sts.get_caller_identity() and None)
    snapshot.error("wrong-code", lambda: sts.get_session_token(SerialNumber=serial, TokenCode="000000"))
    # The enabling codes were spent: wait for the next step's.
    time.sleep(31 - time.time() % 30)
    got = sts.get_session_token(SerialNumber=serial, TokenCode=totp(seed, int(time.time()) // 30))
    assert got["Credentials"]["SessionToken"]


def test_server_certificate_lifecycle(client, names, cleanup, snapshot):
    iam = client("iam")
    name = names("cert")
    made = iam.upload_server_certificate(ServerCertificateName=name, CertificateBody=CERT, PrivateKey=KEY, Path="/conformance/")
    cleanup(iam.delete_server_certificate, ServerCertificateName=name)
    snapshot.match("upload", made, opaque=IDS)
    snapshot.match("get", iam.get_server_certificate(ServerCertificateName=name), opaque=IDS)
    snapshot.match("list", iam.list_server_certificates(PathPrefix="/conformance/"), opaque=IDS,
                   unordered=("ServerCertificateMetadataList",))
    snapshot.match("tag", iam.tag_server_certificate(ServerCertificateName=name, Tags=[{"Key": "k", "Value": "v"}]))
    snapshot.match("tags", iam.list_server_certificate_tags(ServerCertificateName=name))
    renamed = names("cert-2")
    snapshot.match("rename", iam.update_server_certificate(ServerCertificateName=name, NewServerCertificateName=renamed))
    cleanup(iam.delete_server_certificate, ServerCertificateName=renamed)
    snapshot.error("get-old-name", lambda: iam.get_server_certificate(ServerCertificateName=name))
    snapshot.match("delete", iam.delete_server_certificate(ServerCertificateName=renamed))


def test_server_certificate_refusals(client, names, cleanup, snapshot):
    iam = client("iam")
    name = names("cert")
    cleanup(iam.delete_server_certificate, ServerCertificateName=name)
    snapshot.error("key-of-another", lambda: iam.upload_server_certificate(
        ServerCertificateName=name, CertificateBody=CERT, PrivateKey=OTHER_KEY))
    snapshot.error("not-a-certificate", lambda: iam.upload_server_certificate(
        ServerCertificateName=name, CertificateBody="not a certificate", PrivateKey=KEY))
    snapshot.error("not-a-key", lambda: iam.upload_server_certificate(
        ServerCertificateName=name, CertificateBody=CERT, PrivateKey="not a key"))
    snapshot.error("absent", lambda: iam.delete_server_certificate(ServerCertificateName=names("absent")))


def test_signing_certificate_lifecycle(client, names, cleanup, snapshot):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    made = snapshot.match("upload", iam.upload_signing_certificate(UserName=user, CertificateBody=CERT), opaque=IDS)
    cid = made["Certificate"]["CertificateId"]
    cleanup(iam.delete_signing_certificate, UserName=user, CertificateId=cid)
    snapshot.error("same-again", lambda: iam.upload_signing_certificate(UserName=user, CertificateBody=CERT))
    snapshot.match("deactivate", iam.update_signing_certificate(UserName=user, CertificateId=cid, Status="Inactive"))
    snapshot.match("list", iam.list_signing_certificates(UserName=user), opaque=IDS)
    snapshot.error("delete-user-with-certificate", lambda: iam.delete_user(UserName=user))
    snapshot.match("delete", iam.delete_signing_certificate(UserName=user, CertificateId=cid))
    snapshot.error("delete-again", lambda: iam.delete_signing_certificate(UserName=user, CertificateId=cid))


def test_ssh_public_key_lifecycle(client, names, cleanup, snapshot):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    made = snapshot.match("upload", iam.upload_ssh_public_key(UserName=user, SSHPublicKeyBody=SSH_RSA), opaque=IDS)
    kid = made["SSHPublicKey"]["SSHPublicKeyId"]
    cleanup(iam.delete_ssh_public_key, UserName=user, SSHPublicKeyId=kid)
    snapshot.error("same-again", lambda: iam.upload_ssh_public_key(UserName=user, SSHPublicKeyBody=SSH_RSA))
    snapshot.error("ed25519", lambda: iam.upload_ssh_public_key(UserName=user, SSHPublicKeyBody=SSH_ED25519))
    snapshot.match("get-ssh", iam.get_ssh_public_key(UserName=user, SSHPublicKeyId=kid, Encoding="SSH"), opaque=IDS)
    snapshot.match("get-pem", iam.get_ssh_public_key(UserName=user, SSHPublicKeyId=kid, Encoding="PEM"), opaque=IDS)
    snapshot.match("deactivate", iam.update_ssh_public_key(UserName=user, SSHPublicKeyId=kid, Status="Inactive"))
    snapshot.match("list", iam.list_ssh_public_keys(UserName=user), opaque=IDS)
    snapshot.error("delete-user-with-key", lambda: iam.delete_user(UserName=user))
    snapshot.match("delete", iam.delete_ssh_public_key(UserName=user, SSHPublicKeyId=kid))


def test_service_specific_credential_lifecycle(client, names, cleanup, snapshot):
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    made = snapshot.match("create", iam.create_service_specific_credential(
        UserName=user, ServiceName="codecommit.amazonaws.com"), opaque=IDS)
    cid = made["ServiceSpecificCredential"]["ServiceSpecificCredentialId"]
    cleanup(iam.delete_service_specific_credential, UserName=user, ServiceSpecificCredentialId=cid)
    snapshot.error("unsupported-service", lambda: iam.create_service_specific_credential(
        UserName=user, ServiceName="sqs.amazonaws.com"))
    snapshot.match("list", iam.list_service_specific_credentials(UserName=user), opaque=IDS)
    snapshot.match("reset", iam.reset_service_specific_credential(
        UserName=user, ServiceSpecificCredentialId=cid), opaque=IDS)
    snapshot.match("deactivate", iam.update_service_specific_credential(
        UserName=user, ServiceSpecificCredentialId=cid, Status="Inactive"))
    snapshot.error("delete-user-with-credential", lambda: iam.delete_user(UserName=user))
    snapshot.match("delete", iam.delete_service_specific_credential(UserName=user, ServiceSpecificCredentialId=cid))
    snapshot.error("delete-again", lambda: iam.delete_service_specific_credential(
        UserName=user, ServiceSpecificCredentialId=cid))


def test_credential_report(client, snapshot, eventually):
    iam = client("iam")
    # STARTED or COMPLETE depends on the account's last four hours: weather.
    assert iam.generate_credential_report()["State"] in ("STARTED", "INPROGRESS", "COMPLETE")

    def ready():
        from botocore.exceptions import ClientError
        try:
            return iam.get_credential_report()
        except ClientError as e:
            assert e.response["Error"]["Code"] != "ReportInProgress", "still being made"
            raise

    got = eventually(ready, timeout=120)
    lines = got["Content"].decode().splitlines()
    # The columns are the claim; the rows are the account's.
    snapshot.match("columns", {"Header": lines[0], "Format": got["ReportFormat"], "Root": lines[1].split(",")[0]})


ACCESS = json.dumps({"Version": "2012-10-17", "Statement": [
    {"Effect": "Allow", "Action": ["sqs:SendMessage", "s3:Get*", "dynamodb:*"], "Resource": "*"}]})


def test_service_last_accessed_details(client, names, cleanup, snapshot, eventually):
    iam = client("iam")
    role = names("role")
    arn = iam.create_role(RoleName=role, AssumeRolePolicyDocument=json.dumps({
        "Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"},
                                                "Action": "sts:AssumeRole"}]}))["Role"]["Arn"]
    cleanup(iam.delete_role, RoleName=role)
    iam.put_role_policy(RoleName=role, PolicyName="access", PolicyDocument=ACCESS)
    cleanup(iam.delete_role_policy, RoleName=role, PolicyName="access")
    job = iam.generate_service_last_accessed_details(Arn=arn, Granularity="ACTION_LEVEL")["JobId"]

    def done():
        got = iam.get_service_last_accessed_details(JobId=job)
        assert got["JobStatus"] == "COMPLETED", got["JobStatus"]
        return got

    got = eventually(done, timeout=120)
    # A role never used: each allowed service, by name, with no use.
    snapshot.match("services", got, drop=("JobCreationDate", "JobCompletionDate"))
    snapshot.match("with-entities", iam.get_service_last_accessed_details_with_entities(
        JobId=job, ServiceNamespace="sqs"), opaque=("Id",), drop=("JobCreationDate", "JobCompletionDate"))
    snapshot.match("granting", iam.list_policies_granting_service_access(
        Arn=arn, ServiceNamespaces=["sqs", "kms"]))


def test_change_password_doze_only(client, names, cleanup, snapshot, target):
    if target.name == "aws":
        pytest.skip("the recorder may not set console passwords")
    iam = client("iam")
    user = names("user")
    iam.create_user(UserName=user)
    cleanup(iam.delete_user, UserName=user)
    iam.create_login_profile(UserName=user, Password="Initial-pass-1")
    cleanup(iam.delete_login_profile, UserName=user)
    key = iam.create_access_key(UserName=user)["AccessKey"]
    cleanup(iam.delete_access_key, UserName=user, AccessKeyId=key["AccessKeyId"])
    me = client("iam", aws_access_key_id=key["AccessKeyId"], aws_secret_access_key=key["SecretAccessKey"])
    me.change_password(OldPassword="Initial-pass-1", NewPassword="Second-pass-2")
    with pytest.raises(Exception):
        me.change_password(OldPassword="Initial-pass-1", NewPassword="Third-pass-3")
    with pytest.raises(Exception):
        iam.change_password(OldPassword="x", NewPassword="y")


def test_sts_preferences_doze_only(client, target):
    if target.name == "aws":
        pytest.skip("an account-wide setting the recorder may not change")
    iam = client("iam")
    iam.set_security_token_service_preferences(GlobalEndpointTokenVersion="v2Token")
    assert iam.get_account_summary()["SummaryMap"]["GlobalEndpointTokenVersion"] == 2
    iam.set_security_token_service_preferences(GlobalEndpointTokenVersion="v1Token")
