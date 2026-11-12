# doze-aws Rust port: scoreboard

One row per service, filled in at each phase close. Fidelity score = matched / (matched + unverified + deviations) from the
conformance harness. Predictions are written before the measurement; a miss needs a written explanation.

Baseline (Stage 0.3, re-run 2026-11-12): Go doze-aws (origin/main f8c31a1) against AWS recordings, all 17 modules, 850 labels: **423 exact (50%), 225 behavioural differences (26%)**, 18 unverified; the rest differ only in error-message wording. Per-module table in `baseline.md`. Weakest: S3 (33%), KMS (33%), CloudWatch (32%). Strongest: Logs (86%), EventBridge (73%), DynamoDB (68%), Step Functions (68%). The IAM and CloudFormation snapshots are not final (see STATUS.md): their numbers will move when re-recorded.

| Service | Fidelity (Rust) | Fidelity (Go, same snapshots) | Binary delta | Idle RSS delta | Sessions est. | Sessions actual | Audit rounds | Open decisions |
|---|---|---|---|---|---|---|---|---|
| Foundation (A0 to A4) | | | | | 31 to 49 | | | |
| SQS | **177 of 181 labels exact (97%); 98% ignoring wording** (2026-11-12). Long polling and persistence built. The 4 that differ are decided: 2 are the receipt handle quoted in AWS's wording, 1 is Query on SQS (refused by decision), 1 is an AWS sampling quirk | 44% exact / 73% ignoring wording (182 labels) | whole binary 2.56 MiB (macOS arm64), SQS and its database included | idle 3.1 MB against Go's 31.4 MB | 5 to 7 (+1 for redrive tasks, owner decision 2026-11-12; the 7-session trigger stays) | about 3 (A5 up to the tracer bullet, then the rest of the service in one long session) | 0 | the ten-copies quirk; Query protocol on the gateway |
| SSM (Parameter Store) | **27 of 31 labels exact (87%)** (2026-11-12). The 4 that differ are labels where AWS itself answered `AccessDenied` (the recorder's policy allows only `parameter/doze-rec-*`): `reserved-prefix`, `reserved-word`, `reserved-word-starts-a-segment`, `path-without-slash`. They cannot be matched until they are re-recorded under a policy that allows those names | 52% exact / 76% ignoring wording (33 labels) | whole binary 3.09 MB (+399 KB over SQS alone, +21 packages: AES-GCM, regex-lite, the SSM model tables) | idle 3.2 MB (no change from SQS) | 2 to 3 (part of A6's 8 to 11) | about 1 | 0 | SecureString uses the service's own key, not KMS (decide when KMS exists); the 4 AccessDenied labels |
| Secrets Manager | **27 of 27 labels exact (100%)** (2026-11-12) | 48% exact / 81% ignoring wording (27 labels) | whole binary 3.32 MB (+234 KB over SSM, no new dependency) | idle 3.1 MB (no change) | 1 to 2 (part of A6's 8 to 11) | about 1 | 0 | rotation waits for Lambda (A13); `/aws/reference/secretsmanager/*` read-through from SSM waits for the peer bus; secrets are sealed under the service's own key, not KMS |
| KMS | **44 of 45 labels exact (98%)** (2026-11-12). The 1 that differs is `alias/aws/mine`, where AWS itself answered `AccessDenied` under the recorder's policy | 33% exact / 69% ignoring wording (45 labels) | whole binary 3.86 MB (+538 KB and +40 packages over Secrets Manager: ECDSA on four curves, Ed25519, HMAC) | idle 3.2 MB (no change) | 3 to 4 (part of A6's 8 to 11; K2 below is the rest) | about 1 for the part built | 0 | RSA, grants, key import, multi-region replicas, data key pairs, `DeriveSharedSecret` not built (refused by name); the `alias/aws/` error words |
| IAM + STS | | IAM 47% / 60% (45 labels, provisional); STS 50% / 100% (2) | | | | | | |
| SNS | | 64% exact / 74% ignoring wording (39 labels) | | | | | | |
| S3 | | 33% exact / 54% ignoring wording (87 labels) | | | | | | |
| DynamoDB | | 68% exact / 91% ignoring wording (53 labels) | | | | | | |
| Logs | | 86% exact / 93% ignoring wording (28 labels) | | | | | | |
| Kinesis | | 50% exact / 67% ignoring wording (30 labels) | | | | | | |
| CloudWatch | | 32% exact / 50% ignoring wording (28 labels) | | | | | | |
| EventBridge (+ Scheduler) | | 73% exact / 88% ignoring wording (56 labels) | | | | | | |
| Lambda | | 40% exact / 56% ignoring wording (55 labels) | | | | | | |
| Step Functions | | 68% exact / 88% ignoring wording (34 labels) | | | | | | |
| API Gateway | | 53% exact / 72% ignoring wording (36 labels) | | | | | | |
| CloudFormation | | 47% exact / 67% ignoring wording (70 labels, provisional) | | | | | | |

Re-plan triggers: a spike's fallback is taken; SQS API cost exceeds 7 sessions (1.5 times the upper estimate) or its fidelity is below
Go's on the same snapshots; a weight ceiling is breached without a written reason; two consecutive services need more than three
audit rounds.
