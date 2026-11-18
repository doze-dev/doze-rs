"""Where the scenarios run: a doze-aws this file boots, or a real AWS account.

    CONFORMANCE_TARGET=doze   (the default) build the binary, boot it on a free
                              port over an empty data dir, compare.
    CONFORMANCE_TARGET=aws    a real account, to RECORD. Refused unless
                              CONFORMANCE_AWS_ACCOUNT names the account the
                              credentials resolve to.

    CONFORMANCE_ENDPOINT      skip the boot and use a doze-aws already running.

The account check is the one guard that matters. Scenarios create and delete
real resources, and "whatever credentials happened to be in the environment" is
not a decision anybody made. The id has to be typed.
"""

import os
import pathlib
import re
import socket
import subprocess
import tempfile
import time

import boto3
from botocore.config import Config

from .normalize import Normalizer

REPO = pathlib.Path(__file__).resolve().parents[2]

# awsident's fixed local identity.
DOZE_ACCOUNT = "111122223333"
DOZE_REGION = "us-east-1"

# Not acs.amazonaws.com: that is the namespace of S3's ACL groups (`http://acs.amazonaws.com/groups/global/AllUsers`), a fixed
# identifier, not a host either target answers from.
AWS_HOST = re.compile(r"https?://(?!acs\.amazonaws\.com)[a-z0-9.-]+\.amazonaws\.com")


class Target:
    name = ""
    region = ""
    account = ""

    def client(self, service, **kw):
        raise NotImplementedError

    def normalizer(self):
        raise NotImplementedError

    def execute_api_url(self, api_id, stage, path):
        """Where a deployed REST or HTTP API answers."""
        raise NotImplementedError

    def close(self):
        pass


class Doze(Target):
    name = "doze"
    region = DOZE_REGION
    account = DOZE_ACCOUNT

    def __init__(self):
        self.proc = None
        self.tmp = None
        self.endpoint = os.environ.get("CONFORMANCE_ENDPOINT") or self._boot()

    def _boot(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="doze-conformance-")
        root = pathlib.Path(self.tmp.name)
        binary = root / ("doze-aws.exe" if os.name == "nt" else "doze-aws")
        subprocess.run(
            ["go", "build", "-o", str(binary), "./cmd/doze-aws"],
            cwd=REPO, check=True, env={**os.environ, "GOWORK": "off"},
        )
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        # cwd is the temp dir: doze-aws auto-loads ./doze-aws.toml and
        # ./template.yaml, and a run must not depend on where it was started.
        self.log = open(root / "doze-aws.log", "w")
        self.proc = subprocess.Popen(
            [str(binary), "--listen", f"127.0.0.1:{port}", "--data-dir", str(root / "data")],
            cwd=root, stdout=self.log, stderr=subprocess.STDOUT,
        )
        deadline = time.time() + 30
        while time.time() < deadline:
            if self.proc.poll() is not None:
                break
            try:
                socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
                return f"http://127.0.0.1:{port}"
            except OSError:
                time.sleep(0.05)
        self.log.flush()
        raise RuntimeError("doze-aws did not start:\n" + (root / "doze-aws.log").read_text())

    def client(self, service, **kw):
        if service == "cloudwatch" and os.environ.get("CONFORMANCE_CLOUDWATCH_PROTOCOL") == "cbor":
            # botocore prefers the JSON protocol for CloudWatch; this makes it speak RPC v2 CBOR, the other encoding the service
            # offers (and the one newer SDKs choose), so that both can be scored against the same recording.
            import botocore.model

            botocore.model.PRIORITY_ORDERED_SUPPORTED_PROTOCOLS = (
                "smithy-rpc-v2-cbor", *botocore.model.PRIORITY_ORDERED_SUPPORTED_PROTOCOLS)
        return boto3.client(
            service,
            endpoint_url=self.endpoint,
            # A scenario may ask for another region (a replica's).
            region_name=kw.pop("region_name", self.region),
            # A scenario may call as an IAM user it made (its access key).
            aws_access_key_id=kw.pop("aws_access_key_id", "test"),
            aws_secret_access_key=kw.pop("aws_secret_access_key", "test"),
            # One attempt. A retry turns a 500 that happens once into a pass,
            # and here the 500 is the finding.
            config=Config(retries={"total_max_attempts": 1}),
            **kw,
        )

    def normalizer(self):
        # Whoever the test keys are to doze-aws: a refusal names its caller, as AWS's names the recorder's session.
        caller = self.client("sts").get_caller_identity()["Arn"]
        return Normalizer(self.account, endpoints=[self.endpoint], region=self.region, caller=caller)

    def execute_api_url(self, api_id, stage, path):
        # The path form of the execute-api plane: it needs no .doze DNS.
        return f"{self.endpoint}/_aws/execute-api/{api_id}/{stage}/{path.lstrip('/')}"

    def close(self):
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
            self.log.close()
        if self.tmp is not None:
            self.tmp.cleanup()


# KMS keys have no names, so the recorder account's policy lets the role use only keys tagged doze-rec=true. The mark is added to every
# CreateKey here and taken out of every tag listing, so no scenario (and no recording) ever sees it.
_MARK = {"TagKey": "doze-rec", "TagValue": "true"}


def _mark_kms_keys(client):
    def add(params, **_):
        params["Tags"] = [*params.get("Tags", []), _MARK]

    def hide(parsed, **_):
        if "Tags" in parsed:
            parsed["Tags"] = [t for t in parsed["Tags"] if t.get("TagKey") != _MARK["TagKey"]]

    client.meta.events.register("provide-client-params.kms.CreateKey", add)
    client.meta.events.register("after-call.kms.ListResourceTags", hide)


class AWS(Target):
    name = "aws"

    def __init__(self):
        want = os.environ.get("CONFORMANCE_AWS_ACCOUNT", "")
        if not want:
            raise RuntimeError(
                "CONFORMANCE_TARGET=aws creates real resources. Set "
                "CONFORMANCE_AWS_ACCOUNT to the 12-digit account they may be created in."
            )
        self.region = os.environ.get("CONFORMANCE_AWS_REGION", DOZE_REGION)
        self.session = boto3.Session(region_name=self.region)
        me = self.session.client("sts").get_caller_identity()
        self.account, self.caller = me["Account"], me["Arn"]
        if self.account != want:
            raise RuntimeError(
                f"the credentials in this environment belong to account {self.account}, "
                f"and CONFORMANCE_AWS_ACCOUNT allows only {want}. Nothing was created."
            )

    def client(self, service, **kw):
        # A short connect timeout: where an endpoint's IPv6 addresses do not answer (seen 2026-11-15 on the recorder's network),
        # botocore tries each one for 60 s before IPv4, and the request reaches AWS after its signature has expired.
        if "config" not in kw:
            from botocore.config import Config
            kw["config"] = Config(connect_timeout=5)
        client = self.session.client(service, **kw)
        if service == "kms":
            _mark_kms_keys(client)
        return client

    def normalizer(self):
        return Normalizer(self.account, endpoint_patterns=[AWS_HOST], region=self.region, caller=self.caller)

    def execute_api_url(self, api_id, stage, path):
        return f"https://{api_id}.execute-api.{self.region}.amazonaws.com/{stage}/{path.lstrip('/')}"


def open_target():
    which = os.environ.get("CONFORMANCE_TARGET", "doze")
    if which == "doze":
        return Doze()
    if which == "aws":
        return AWS()
    raise RuntimeError(f"CONFORMANCE_TARGET={which!r}: expected doze or aws")
