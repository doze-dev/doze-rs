"""STS through boto3. Small on purpose: who you are is the one thing a local
emulator and a real account are supposed to disagree about."""

import base64
import hashlib
import http.server
import json
import threading
import time

import pytest
from botocore.exceptions import ClientError


def test_caller_identity_has_the_shape(client, snapshot):
    snapshot.match("identity", client("sts").get_caller_identity(), opaque=("UserId", "Arn"))


def test_refusals(client, snapshot, target):
    sts = client("sts")
    # Not a length: botocore checks lengths itself and never sends the call.
    # A pattern only the service can refuse.
    snapshot.error("bad-session-name", lambda: sts.assume_role(
        RoleArn=f"arn:aws:iam::{target.account}:role/does-not-matter",
        RoleSessionName="has spaces"))


# --- AssumeRoleWithWebIdentity against a local identity provider (doze-aws only) -----------------------------------------------
#
# AWS cannot reach an identity provider on this machine, so this scenario is UNRECORDABLE: it runs on the doze target only, and
# asserts behaviour rather than holding labels. The provider is a plain http server on 127.0.0.1 serving a discovery document and a
# JWKS with one RSA key; doze-aws fetches an issuer's keys over http:// only on this machine (crates/aws-iam/src/oidc.rs). Tokens
# are signed RS256 with a throwaway key made for this test (openssl genrsa 2048), in pure Python: the harness has boto3 alone.


KEY_N = 0xbe1a2c6245059d9ba6b925f0d4a2d69389baffd98b280df533cc16a9de237793f1db0f55035aca5f32abdc94327d4b2ac88b61803a613570026862e393a9ed283bf9aabe6738200fbf06f08394470d0799561c8bfe3703201fe1fa68055e41be25edd38b1bb30641f0101bbee76c51924a2603019d2185211d9eb4ab3eba1a1a76d3d5f58639b5007eafe4474269b6d901b4f7e3e59426ca56bc120615b094bdcfd2d12b1d590af903106d33c12a3cf3823a99bdb98cf7dd8787286a726ae3da94db64976e96d758cc507840ea09a5f923a3a8fad0fdb57c9e1f58dc649bc50b7e9df29e86d51df74b4e942b05d35eda37fc52e8dd732e6c9310d8f9934d2c63
KEY_D = 0x6021b2321b7f447d32a94c136623d668936c99f6f394d4c21b6868498c48d92e5a4e91894702017674944e1e2f0e0715e286014f2bb67ea45ec354c68aa492861716210ed9f99ad4fb9fd1be681c29ed9ecfd9edbd272a90732efe46ddf8153cc7c7658362057cf399273c30a4acb4ef4a49c0996f51451642f93f999d45d4ee8f560e84df3685fd29e72b97c3e411ec4edc0bc60b4d4036e647547eee35ff16da3e37534e63adb1b0385f81e8dfad73b0b6021aef187dfcb248da3092a1f966aa30c8cb68fd6a45eaf99c1477a9c8e0216559a7d47c1263cde740d79563330e1c08bc6904958f8dab6cb382b15affc51b6afc9ebd92f284547f2d47bfc82c51
# DigestInfo for SHA-256 (RFC 8017, 9.2 note 1).
SHA256_INFO = bytes.fromhex("3031300d060960864801650304020105000420")


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def rs256(signed):
    size = (KEY_N.bit_length() + 7) // 8
    info = SHA256_INFO + hashlib.sha256(signed).digest()
    em = b"\x00\x01" + b"\xff" * (size - len(info) - 3) + b"\x00" + info
    return pow(int.from_bytes(em, "big"), KEY_D, KEY_N).to_bytes(size, "big")


def token(claims, kid="k1"):
    head = b64url(json.dumps({"alg": "RS256", "kid": kid, "typ": "JWT"}).encode())
    body = b64url(json.dumps(claims).encode())
    signed = f"{head}.{body}".encode()
    return f"{head}.{body}.{b64url(rs256(signed))}"


@pytest.fixture
def local_idp(target):
    if target.name == "aws":
        pytest.skip("AWS cannot reach an identity provider on this machine")
    n = KEY_N.to_bytes((KEY_N.bit_length() + 7) // 8, "big")
    jwk = {"kty": "RSA", "kid": "k1", "use": "sig", "alg": "RS256", "n": b64url(n), "e": "AQAB"}

    class Idp(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            base = f"http://127.0.0.1:{self.server.server_port}"
            if self.path == "/.well-known/openid-configuration":
                doc = {"issuer": base, "jwks_uri": base + "/jwks"}
            elif self.path == "/jwks":
                doc = {"keys": [jwk]}
            else:
                self.send_error(404)
                return
            out = json.dumps(doc).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)

        def log_message(self, *args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Idp)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{server.server_port}"
    server.shutdown()


def test_web_identity_needs_a_token_its_provider_signed(client, names, cleanup, target, local_idp):
    iam, sts = client("iam"), client("sts")
    made = iam.create_open_id_connect_provider(Url=local_idp, ClientIDList=["doze-conformance"])
    cleanup(iam.delete_open_id_connect_provider, OpenIDConnectProviderArn=made["OpenIDConnectProviderArn"])
    role = f"arn:aws:iam::{target.account}:role/{names('web')}"
    claims = {"iss": local_idp, "sub": "alice", "aud": "doze-conformance", "exp": int(time.time()) + 600}
    good = token(claims)

    def assume(t):
        return sts.assume_role_with_web_identity(RoleArn=role, RoleSessionName="web", WebIdentityToken=t)

    got = assume(good)
    assert got["SubjectFromWebIdentityToken"] == "alice"
    assert got["Audience"] == "doze-conformance"

    def refused(t):
        with pytest.raises(ClientError) as e:
            assume(t)
        return e.value.response["Error"]["Code"]

    head, body, sig = good.split(".")
    raw = bytearray(base64.urlsafe_b64decode(sig + "=" * (-len(sig) % 4)))
    raw[-1] ^= 1
    assert refused(f"{head}.{body}.{b64url(bytes(raw))}") == "InvalidIdentityToken"
    forged = b64url(json.dumps({**claims, "sub": "mallory"}).encode())
    assert refused(f"{head}.{forged}.{sig}") == "InvalidIdentityToken"
    assert refused(f"{head}.{body}.") == "InvalidIdentityToken"
    assert refused(token(claims, kid="nope")) == "InvalidIdentityToken"
    assert refused(token({**claims, "aud": "someone-else"})) == "InvalidIdentityToken"
    assert refused(token({**claims, "exp": int(time.time()) - 60})) == "ExpiredTokenException"
