# The AWS service models

Smithy JSON AST (`"smithy": "2.0"`) models of 20 AWS services, the input of `cargo xtask codegen`. They are AWS's published
API models (the `aws/api-models-aws` repository, Apache-2.0, the same models the AWS SDKs are generated from), kept verbatim:
nothing here is edited. Constraints the models lack (the SQS model has none at all) live in `codegen/overlays/`, each with the
evidence it comes from.

**Where they came from is not fully recorded.** They were fetched on 2026-10-10 for the Go edition's audit (`doze-aws/.audit-models`,
untracked there) and the upstream revision was not written down. The checksums below pin what this repository was built from.
If they are refreshed, re-run `cargo xtask codegen`, read the diff of the generated crates, and update this file.

`dynamodb-streams.json` is the exception: it was not in the Go audit's set and was fetched on 2026-11-13 from
`models/dynamodb-streams/service/2012-08-10/dynamodb-streams-2012-08-10.json` at upstream commit
`f231310cc68b985a403274fb88f0f3856fe3b52f` (the `main` branch that day).

`scheduler.json` (EventBridge Scheduler) was fetched on 2026-11-14 from `models/scheduler/service/2021-06-30/scheduler-2021-06-30.json` at
upstream commit `46135f8468cd5cb60d983b97290c9f6661e359c6` (the `main` branch that day). It is an older-style model (prelude member
targets such as `smithy.api#String`, and the Smithy 1.0 `@enum` trait), which `codegen/src/legacy.rs` reads; nothing in it is edited.

`apigatewaymanagementapi.json` (the WebSocket `@connections` API) was fetched on 2026-11-14 from
`models/apigatewaymanagementapi/service/2018-11-29/apigatewaymanagementapi-2018-11-29.json` at upstream commit
`46135f8468cd5cb60d983b97290c9f6661e359c6` (the `main` branch that day).

`s3.reference.json` of the Go audit (a derived reference, not a model) is not carried over.

| file | sha256 |
|---|---|
| apigatewaymanagementapi.json | 43d92c4c358869b6422c2870f5d7e8cc7ea26c6f0c2f13c2264c6606e37d0c55 |
| api-gateway.json | 51f7bb0a8c434669394c0887bacefaefdae66994122229fbac87e6d0f1da859a |
| apigatewayv2.json | e10bcf53a23a41be2d4a255087e87effb1cf5c29756a54ae7b92fbee8d892f96 |
| cloudformation.json | 3de1f8d8357291665ddbced1780998a84d15b0df2264c2aedd98fc09a9189f7d |
| cloudwatch-logs.json | 308aa3e4d27c48f1233a6c286e970948a64a9eb157ee446b8227364eb7d8c49b |
| cloudwatch.json | b1abe105489d574be25364ad5cc4a67f5782f4cc6bcf6439a587625bacc9c62a |
| dynamodb.json | 889458e11643e5f4b240af1d2f1fd0271d06ebf44d88fb571853aa5c8e1ae438 |
| dynamodb-streams.json | a5723166fba32c1a59e8de923a0919c4e959645722e4f48f728ac3fded793332 |
| eventbridge.json | 55dc4532838b71926afff18b6ce7a11892c6f4aadc81f4036b983eea24c4a449 |
| iam.json | c0cec5eeceeffdf03f310ae41c30d072041dbc274bc5a388cad525847de1bec2 |
| kinesis.json | b3cd88f396f3591df7f1a809957379a9c8afba1e593e3eb38ee7d99afb689636 |
| kms.json | 82e414ec6095a69dabe0e7de071a5b9704c413d3277ed1b293d3cbcd8dd6ce5f |
| lambda.json | 8d1ae738f4deb32415e1871dc1ed678b8ee12a087438f309feaf0b813cba04e1 |
| s3.json | 57431518807d8565e103b9bcc8f2610950e725645e28374e746bdf8e300cc37a |
| scheduler.json | 275f9e955563b99984d052e384b27b65b48a2f3a3a1d3a1c9b82c95e6f5d9dd1 |
| secrets-manager.json | b1bce4d92db3876e6811dd5900a26e4b6b9af15833425dc7a07a5e23886fa886 |
| sfn.json | 2ef5bf10bf1b248a34d313f6bd6334e9f03e6eebf5c35784ab714ba01cca508a |
| sns.json | 18f6b7a336227cb46df17f20108498357dbe79f255cf3bad759d0d58f6d0bcbf |
| sqs.json | 38eef06ce5642b8a052282fd19a3ab275e0046d375474e2ff022d3e33aaeab72 |
| ssm.json | 8aac35e07b6511821a0704008decdc8a0b69bc8deeb6ab3efadc5426a3225ef7 |
| sts.json | 3012bfe1c6d44b4348ed03888085fd0b7a506e4da3335250280537b85e834864 |
