# iam-ops: the 41 planned IAM operations (branch `aws-iam-ops`, 2026-11-18)

On `origin/main` at `f864c8dd9`. The integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`; none of those was
touched here. AWS is the only reference: each behaviour cites the model (`models/iam.json`, `models/sts.json`) or is marked
`(memory)` in the code beside it. **Nothing new here is recorded**: the `doze-rec-main` SSO session had expired, so every new boto3
label is UNVERIFIED until the integrator records `conformance/tests/test_iam_credentials.py`.

All 41 are built; none moved to `not_planned`. IAM is now 157 of 180 (the 23 left are the two not-planned families).

## 0. Storage (`held.rs`, `doze-store`)

- Everything new is in tables of its own (`mfa`, `server-certificates`, `user-credentials`, `passwords`, `reports`, `jobs`,
  `last-accessed`, `preferences`), so no existing record changed shape. `Records::load_or_new` (new in `doze-store`) reads a table
  a later version added as empty when the file predates it: an `iam.redb` made before this stage opens (test
  `a_file_made_before_the_newer_tables_opens_and_takes_them`; `doze-store/tests/new_table.rs`). The other services' added tables
  (SSM documents, KMS sets) did not have this and would refuse an older file; they can use it too.
- `doze-aws data` counts and resets the new tables; a reset clears them.

## 1. Virtual MFA (`mfa.rs`, `totp.rs`)

- `CreateVirtualMFADevice` makes a 20-byte seed and answers it as base32 (RFC 4648, the blob the model describes); the serial is
  `arn:aws:iam::<account>:mfa<path><name>`. **No QR code PNG** (a difference: drawing one needs a QR encoder and PNG writer, and
  every tool also accepts the seed). `EnableMFADevice` and `ResyncMFADevice` need two consecutive codes (RFC 6238, HMAC-SHA-1, 30 s,
  six digits; tested against RFC 6238 appendix B). Windows: enable accepts codes up to ten steps old, resync thirty minutes either
  way (memory). A code, once accepted, is spent (the step is kept), so it cannot open a second session.
- `Deactivate`, `DeleteVirtual` (refused while assigned), `ListMFADevices` (the caller's when no user: the root has none here),
  `ListVirtualMFADevices` (by assignment; no seed), tags. Eight devices per user (memory). `GetAccountSummary` counts devices.
- `GetMFADevice`: the model says "we only accept FIDO security key ARNs"; FIDO keys cannot be registered locally, so it answers
  `NoSuchEntity` with a sentence pointing at `ListMFADevices`.
- **STS**: `GetSessionToken` and `AssumeRole` with `SerialNumber` and `TokenCode` check the code against a device enabled for the
  calling user (one step either way, unspent) and refuse with `AccessDenied` in AWS's two sentences (memory) otherwise, whatever
  `--iam` is. The session remembers when MFA was given: `aws:MultiFactorAuthPresent` is `true`/`false` and
  `aws:MultiFactorAuthAge` set for temporary credentials, absent for long-term keys (memory: the condition-key reference); they are
  no longer in the "unsupplied" list. For `AssumeRole`, a good code also puts both keys in the trust policy's context, so a trust
  policy that asks for MFA is met (test `a_trust_policy_that_asks_for_mfa_is_met_by_a_code`).
- Fixed on the way: a `GetSessionToken` session was evaluated as a role session with no policies (everything denied under
  `enforce`); it is now its user, with the user's policies.

## 2. Certificates (`certs.rs`, `x509.rs`, `der.rs`)

- PEM X.509 walked by hand (`der.rs`, shared with the SAML metadata reader) to its validity (UTCTime and GeneralizedTime) and its
  `subjectPublicKeyInfo`. A body that is not exactly one certificate, or has expired, is `MalformedCertificate` (the model: "malformed
  or expired"). A server certificate's private key (PKCS #1 or PKCS #8 RSA; SEC 1 or PKCS #8 EC P-256/P-384, through the crates KMS
  already links) must be the certificate's: `KeyPairMismatch` otherwise; a key that does not read is `MalformedCertificate` (memory).
  A chain must be certificates only. Twenty server certificates per account (the summary's quota); id `ASCA...`; rename moves the ARN.
- Signing certificates: two per user (the summary's quota), the same certificate once in the account (`DuplicateCertificate`), ids of
  32 upper-case letters and digits (memory), Active/Inactive.
- Fuzz target `fuzz_certificate` (`crates/aws-iam/fuzz`).

## 3. SSH keys and service-specific credentials (`usercreds.rs`, `sshkey.rs`)

- SSH: `ssh-rsa` or PEM (SPKI `PUBLIC KEY` or PKCS #1 `RSA PUBLIC KEY`), read by hand to modulus and exponent; at least 2048 bits
  (model); other key types `UnrecognizedPublicKeyEncoding`; the fingerprint is the MD5 of the SSH form, colon-separated (what
  `ssh-keygen -l -E md5` prints; the model says 48 characters and this is 47: open, needs a recording). `GetSSHPublicKey` writes the
  key in either encoding. Five per user (memory). Fuzz target `fuzz_ssh_key`.
- Service-specific credentials: CodeCommit and Keyspaces get `<user>-at-<account>` (model) and a password; Bedrock and CloudWatch
  Logs get an API key (alias and secret, with `CredentialAgeDays` an expiry) (memory: their shapes). Two per service per user
  (model). Reset makes a new password or secret. Kept only: doze-aws runs none of these services.

## 4. Passwords (`password.rs`)

- `CreateLoginProfile` and `UpdateLoginProfile` now keep the password (PBKDF2-HMAC-SHA-256, 4096 rounds, salted) and hold it to
  the account's password policy when one is set: length, the four character classes, and `PasswordReusePrevention` (up to 24 kept)
  (memory: AWS's sentences). `PasswordResetRequired` is kept and answered by `GetLoginProfile` (it was always `false`).
- `ChangePassword` is the caller's own: the root or a role is `InvalidUserType` (memory); the old password must match. **AWS's
  answer to a wrong old password is not known**: `AccessDenied` here is a guess, marked in the code, and an open question. A
  password set before this stage cannot be checked, and any old password is taken.

## 5. A user's credentials and `DeleteUser` (`userheld.rs`)

`DeleteUser` now refuses while the user has a login profile, an MFA device, signing certificates, SSH keys or service-specific
credentials, in that order, after the recorded groups and keys: `Cannot delete entity, must delete <what> first.` (memory: the
order and the words; "login profile" and "MFA device" are widely quoted). A renamed user keeps them all. CloudFormation already
deletes the login profile before the user.

## 6. Credential report (`report.rs`)

CSV in the IAM user guide's 22 columns, a root row (`<root_account>`, `not_supported` where IAM cannot say) and one row per user,
times as `2026-11-18T09:30:00+00:00` (memory: the guide's format). One report per four hours: a `Generate` within four hours of the
last answers `COMPLETE` and makes nothing new, after that the old one is `CredentialReportExpired` (memory). Made at once
(`STARTED` the first time, never `INPROGRESS`). A console password is never used locally: `no_information`. The root's creation
time is when this IAM first made a report (kept in `preferences`).

## 7. Last-accessed data (`lastaccess.rs`, `grants.rs`)

- The authorizer notes every question an IAM user or role (by its sessions) asks, allowed or not (the model: "all attempts"),
  per entity and service and action, written at most once a minute each, as access keys' last use already was. Cheap: it rides the
  write the authorizer already makes. The root and keys IAM did not issue are not noted, and `--iam off` notes nothing.
- `GenerateServiceLastAccessedDetails` snapshots the services the entity's (or policy's) `Allow` statements name, and their use by
  the user, the role, a group's members, or a policy's holders; `Get...` answers `COMPLETED` at once, paged; action level lists
  every action asked for (AWS tracks a few services' actions). `...WithEntities` lists the entities newest first.
  `ListPoliciesGrantingServiceAccess`: the user's and its groups' policies, inline and managed, per namespace.
- Wildcards in a service part (`*`, `s3*:...`, `NotAction`) are matched against the 19 namespaces doze-aws runs (AWS: all of its
  services). Their `ServiceName`s are (memory): the recorded scenario will settle them.
- The 100 most recent jobs are kept; a deleted user or role takes its use with it. The job id is not tied to its caller (AWS ties
  it to the same user or role session: not enforced).

## 8. `SetSecurityTokenServicePreferences`

Kept and shown as `GlobalEndpointTokenVersion` in `GetAccountSummary`; doze-aws has one STS endpoint and one token form.

## Scenarios (`conformance/tests/test_iam_credentials.py`, all UNVERIFIED: 56 labels)

MFA lifecycle and refusals, a session token with a real code, server certificates (lifecycle, refusals), signing certificates, SSH
keys, service-specific credentials, the credential report's columns, last-accessed details on a fresh role. `ChangePassword` and
`SetSecurityTokenServicePreferences` run against doze-aws only (the recorder is denied `CreateLoginProfile` and the account-wide
setting). The harness's doze target now takes a scenario's own access key (`client("sts", aws_access_key_id=...)`). Against this
branch's build: the 12 new scenarios pass; `test_iam.py` and `test_sts.py` still match all 50 recorded labels
(re-run 2026-11-19 with `cargo xtask conformance --cmd "doze-aws --account 111122223333 ..."`: iam 48/48, sts 2/2 exact,
iam_credentials 56 unverified; the SSO session was still expired, so nothing was recorded and nothing was made on AWS).

## Open

- Record `test_iam_credentials.py` (needs a fresh SSO login); every (memory) above, especially: the `DeleteUser` order and words,
  `ChangePassword`'s wrong-password answer, the SSH fingerprint's length, the service names, the credential report's root row.
- The QR code PNG.
- The last-accessed job's tie to its caller.
