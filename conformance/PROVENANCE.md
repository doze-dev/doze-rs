# Provenance

This directory is a fork of `doze-aws/conformance` (Go repository, origin/main f8c31a1, 2026-11-09). The Go repository is
frozen; the Rust edition owns its oracle from here on.

Changes from the original:

- `CONFORMANCE_NAME_PREFIX` (default `doze-rec-`) replaces the hard-coded `dzc-` in resource names, so the recorder account's
  IAM policy (names starting with `doze-rec-` only) applies. See `docs/port/aws/phase0.md`.
- Scenarios corrected where AWS contradicted the original author (each has a comment with the date): SQS
  `test_invalid_policy` (no Principal is accepted), `test_move_task_cancel` (a second cancel is answered), and
  `test_add_permission_rules` (AWS verifies the principal account exists and accepts any label).
- Snapshots in `snapshots/` are recordings from real AWS (the recorder account, us-east-1; its id is kept out of the repository) made with
  `CONFORMANCE_TARGET=aws CONFORMANCE_RECORD=1`.

Run against a server that is already up (the Go-only boot path is not used here):

    CONFORMANCE_ENDPOINT=http://127.0.0.1:PORT uv run --frozen pytest tests/test_sqs.py

Record (needs `aws sso login --profile doze-rec-main`, and `tools/aws-rec/teardown.ts` afterwards):

    AWS_PROFILE=doze-rec-main CONFORMANCE_TARGET=aws CONFORMANCE_AWS_ACCOUNT=$DOZE_REC_ACCOUNT CONFORMANCE_RECORD=1 \
      uv run --frozen pytest tests/test_<service>.py
