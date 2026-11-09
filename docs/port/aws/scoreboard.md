# doze-aws Rust port: scoreboard

One row per service, filled in at each phase close. Fidelity score = matched / (matched + unverified + deviations) from the
conformance harness. Predictions are written before the measurement; a miss needs a written explanation.

Baseline (filled in at Stage 0.3): Go doze-aws against AWS snapshots: **not measured yet**.

| Service | Fidelity (Rust) | Fidelity (Go, same snapshots) | Binary delta | Idle RSS delta | Sessions est. | Sessions actual | Audit rounds | Open decisions |
|---|---|---|---|---|---|---|---|---|
| Foundation (A0 to A4) | | | | | 31 to 49 | | | |
| SQS | | | | | 5 to 7 | | | |
| SSM | | | | | | | | |
| Secrets Manager | | | | | | | | |
| KMS | | | | | | | | |
| IAM + STS | | | | | | | | |
| SNS | | | | | | | | |
| S3 | | | | | | | | |
| DynamoDB | | | | | | | | |
| Logs | | | | | | | | |
| Kinesis | | | | | | | | |
| CloudWatch | | | | | | | | |
| EventBridge (+ Scheduler) | | | | | | | | |
| Lambda | | | | | | | | |
| Step Functions | | | | | | | | |
| API Gateway | | | | | | | | |
| CloudFormation | | | | | | | | |

Re-plan triggers: a spike's fallback is taken; SQS API cost exceeds 7 sessions (1.5 times the upper estimate) or its fidelity is below
Go's on the same snapshots; a weight ceiling is breached without a written reason; two consecutive services need more than three
audit rounds.
