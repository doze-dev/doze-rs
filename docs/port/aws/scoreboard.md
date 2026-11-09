# doze-aws Rust port: scoreboard

One row per service, filled in at each phase close. Fidelity score = matched / (matched + unverified + deviations) from the
conformance harness. Predictions are written before the measurement; a miss needs a written explanation.

Baseline (Stage 0.3, in progress): Go doze-aws (origin/main f8c31a1) against AWS recordings, 10 modules, 537 labels: **249 exact (46%), 147 behavioural differences (27%)**; the rest differ only in error-message wording or are not yet recorded. Per-module table in `baseline.md`. Weakest: S3 and CloudWatch. Strongest: DynamoDB. Logs, KMS, IAM, Lambda, CloudFormation, API Gateway and Step Functions are not recorded yet.

| Service | Fidelity (Rust) | Fidelity (Go, same snapshots) | Binary delta | Idle RSS delta | Sessions est. | Sessions actual | Audit rounds | Open decisions |
|---|---|---|---|---|---|---|---|---|
| Foundation (A0 to A4) | | | | | 31 to 49 | | | |
| SQS | | 44% exact / 73% ignoring wording (182 labels) | | | 5 to 7 | | | |
| SSM | | 52% exact / 76% ignoring wording (33 labels) | | | | | | |
| Secrets Manager | | 48% exact / 81% ignoring wording (27 labels) | | | | | | |
| KMS | | | | | | | | |
| IAM + STS | | 50% exact / 100% ignoring wording (2 labels) | | | | | | |
| SNS | | 64% exact / 74% ignoring wording (39 labels) | | | | | | |
| S3 | | 33% exact / 54% ignoring wording (87 labels) | | | | | | |
| DynamoDB | | 68% exact / 91% ignoring wording (53 labels) | | | | | | |
| Logs | | | | | | | | |
| Kinesis | | 50% exact / 67% ignoring wording (30 labels) | | | | | | |
| CloudWatch | | 32% exact / 50% ignoring wording (28 labels) | | | | | | |
| EventBridge (+ Scheduler) | | 43% exact / 57% ignoring wording (56 labels) | | | | | | |
| Lambda | | | | | | | | |
| Step Functions | | | | | | | | |
| API Gateway | | | | | | | | |
| CloudFormation | | | | | | | | |

Re-plan triggers: a spike's fallback is taken; SQS API cost exceeds 7 sessions (1.5 times the upper estimate) or its fidelity is below
Go's on the same snapshots; a weight ceiling is breached without a written reason; two consecutive services need more than three
audit rounds.
