"""Where doze-aws differs from AWS on purpose.

A difference the suite finds is a bug until somebody decides otherwise. This
file is where that decision is written down: the scenario and label, and the
reason. A listed difference does not fail the run; it is counted and printed
in the summary every time, so it stays a decision rather than becoming a habit.

It ratchets in both directions. A difference that is not listed fails. A
listing that no longer differs fails too — fix the behaviour and the entry has
to go with it.

Key: "<module>::<test>::<label>".
"""

# Meant to stay nearly empty: doze-aws is not more forgiving than AWS.
# What belongs here is only what cannot exist on a laptop at all, or what an
# owner-chosen default trades for local ease while a flag gives AWS's answer.
DEVIATIONS = {
    # AWS reads the last invocation's EndTimestamp apart from the execution's (two clock readings in its own pipeline); doze-aws
    # ends both at one instant, and faking an offset would be inventing data. Listed in docs/aws/compatibility-notes.toml.
    "test_lambda_durable::test_durable_execution_with_a_wait_and_a_callback::history-newest-first":
        "the last invocation ends at the instant the execution does; AWS takes two readings",
    # AWS answers a virtual-hosted URL on its own host (https://<bucket>.s3.amazonaws.com/<key>); that host cannot exist locally,
    # so doze-aws answers the URL that fetches the object from itself (<endpoint>/<bucket>/<key>, path style).
    "test_s3::test_multipart_upload::complete":
        "Location names doze-aws's own address: AWS's S3 host does not exist on a laptop",
    # The default gateway trusts signatures (it has no secret keys to check them with), so a SigV2 presigned PUT whose signature
    # does not cover the Content-Type urllib adds is served. `--auth verify` refuses it with 403 as AWS does (aws-auth sigv2 tests).
    "test_s3::test_presigned_urls::put":
        "signatures are trusted by default; --auth verify refuses this PUT as AWS does",
    "test_s3::test_presigned_urls::what-the-put-wrote":
        "follows from the trusted PUT above: the object exists locally, not on AWS",
    # `--iam soft` (the default) logs a refusal and serves the request; `--iam enforce` refuses the anonymous read as AWS does.
    "test_s3::test_anonymous_reads_need_a_public_grant::anonymous-denied":
        "--iam soft (the default) logs the refusal and serves; --iam enforce refuses as AWS does",
}
