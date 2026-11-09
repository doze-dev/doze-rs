# aws-rec: the recorder account's tools

The recorder account is the main AWS account, reached through IAM Identity Center with the permission set
`doze-recorder-limited` (profile `doze-rec-main`; `aws sso login --profile doze-rec-main` every 8 hours). No access keys exist.

- `teardown.ts`: deletes everything named `doze-rec-*` (and KMS keys tagged `doze-rec=true`). Dry run by default; `--delete` deletes
  and then lists again. `--only sqs,s3` restricts it. Run it after every recording session. It refuses to run in another account.
- `seed-test.ts`: creates throwaway `doze-rec-td-*` resources to test the teardown.
- `recorder-permission-set.ts`: generates the inline policy of the permission set (`bun recorder-permission-set.ts` writes the JSON).
- `recorder-scp.json`: a service control policy for a dedicated member account. Not used now (the member account is parked).

The Identity Center KMS key in the account is not ours and the policy denies touching it.
