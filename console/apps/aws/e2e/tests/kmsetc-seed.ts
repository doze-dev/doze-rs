// The world the KMS, Secrets Manager and Parameter Store boards are drawn in ("larder"), made with the real AWS CLI against a
// doze-aws: IAM users and roles to put in key policies, five customer managed keys (one disabled, one pending deletion, two
// asymmetric, one with no alias and no tags), the queue that uses the first, two secrets (one rotated by a Lambda function, with a
// resource policy and two versions) and eight parameters (one SecureString with three versions and labels, one Advanced).
// `bun e2e/tests/kmsetc-seed.ts http://127.0.0.1:46290 [account]` seeds a running one by hand.
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws } from "../server";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

const TRUST = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });

/** The rotation function: the four steps of a rotation, made with plain HTTP calls to the endpoint the function is given. */
export const ROTATE = `import json, os, urllib.request

def call(op, body):
    key = os.environ.get("AWS_ACCESS_KEY_ID", "AKIAROTATE")
    req = urllib.request.Request(os.environ["AWS_ENDPOINT_URL"], data=json.dumps(body).encode(), method="POST", headers={
        "X-Amz-Target": "secretsmanager." + op, "Content-Type": "application/x-amz-json-1.1", "X-Amz-Date": "20261101T000000Z", "X-Amz-Security-Token": os.environ.get("AWS_SESSION_TOKEN", ""),
        "Authorization": "AWS4-HMAC-SHA256 Credential=%s/20261101/us-east-1/secretsmanager/aws4_request, SignedHeaders=host, Signature=00" % key})
    return json.load(urllib.request.urlopen(req))

def lambda_handler(event, context):
    arn, token, step = event["SecretId"], event["ClientRequestToken"], event["Step"]
    if step == "createSecret":
        cur = call("GetSecretValue", {"SecretId": arn, "VersionStage": "AWSCURRENT"})
        call("PutSecretValue", {"SecretId": arn, "ClientRequestToken": token, "SecretString": cur["SecretString"], "VersionStages": ["AWSPENDING"]})
    elif step == "finishSecret":
        stages = call("DescribeSecret", {"SecretId": arn})["VersionIdsToStages"]
        current = [v for v, s in stages.items() if "AWSCURRENT" in s][0]
        call("UpdateSecretVersionStage", {"SecretId": arn, "VersionStage": "AWSCURRENT", "MoveToVersionId": token, "RemoveFromVersionId": current})
`;

function zipOf(file: string, source: string): string {
  const dir = mkdtempSync(join(tmpdir(), "kmsetc-seed-"));
  writeFileSync(join(dir, file), source);
  const zip = join(dir, "fn.zip");
  if (Bun.spawnSync(["zip", "-q", "-j", zip, join(dir, file)]).exitCode !== 0) throw new Error("zip failed");
  return zip;
}

/** What the console writes into a key policy for the account, one administrator and two users. */
export function keyPolicy(account: string): string {
  const arn = (kind: string, name: string) => `arn:aws:iam::${account}:${kind}/${name}`;
  const users = [arn("role", "lambda-exec"), arn("role", "report-job")];
  return JSON.stringify({
    Version: "2012-10-17",
    Id: "key-consolepolicy-3",
    Statement: [
      { Sid: "Enable IAM User Permissions", Effect: "Allow", Principal: { AWS: `arn:aws:iam::${account}:root` }, Action: "kms:*", Resource: "*" },
      { Sid: "Allow access for Key Administrators", Effect: "Allow", Principal: { AWS: arn("role", "larder-admin") }, Action: ["kms:Create*", "kms:Describe*", "kms:Enable*", "kms:List*", "kms:Put*", "kms:Update*", "kms:Revoke*", "kms:Disable*", "kms:Get*", "kms:Delete*", "kms:TagResource", "kms:UntagResource", "kms:ScheduleKeyDeletion", "kms:CancelKeyDeletion", "kms:RotateKeyOnDemand"], Resource: "*" },
      { Sid: "Allow use of the key", Effect: "Allow", Principal: { AWS: users }, Action: ["kms:Encrypt", "kms:Decrypt", "kms:ReEncrypt*", "kms:GenerateDataKey*", "kms:DescribeKey"], Resource: "*" },
      { Sid: "Allow attachment of persistent resources", Effect: "Allow", Principal: { AWS: users }, Action: ["kms:CreateGrant", "kms:ListGrants", "kms:RevokeGrant"], Resource: "*", Condition: { Bool: { "kms:GrantIsForAWSResource": true } } },
    ],
  });
}

export const secretPolicy = (account: string) => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { AWS: `arn:aws:iam::${account}:role/larder-admin` }, Action: "secretsmanager:GetSecretValue", Resource: "*" }] });

export const STRIPE = '{"secret_key":"sk_live_EXAMPLE_a8Fq2x","webhook_secret":"whsec_EXAMPLE_k3Lm9p"}';

/** Makes the larder world; the account is the one the doze-aws serves. */
export function seedKmsetc(endpoint: string, account: string): void {
  const cli = runner(endpoint);
  for (const r of ["lambda-exec", "larder-admin", "report-job"]) cli(["iam", "create-role", "--role-name", r, "--assume-role-policy-document", TRUST]);
  for (const u of ["ci-bot", "dev"]) cli(["iam", "create-user", "--user-name", u]);

  // Keys.
  const key = (args: string[]) => (JSON.parse(cli(["kms", "create-key", ...args])) as { KeyMetadata: { KeyId: string } }).KeyMetadata.KeyId;
  const app = key(["--description", "Encrypts larder queues and secrets", "--policy", keyPolicy(account), "--tags", "TagKey=Project,TagValue=larder", "TagKey=Env,TagValue=prod", "TagKey=team,TagValue=fulfilment"]);
  cli(["kms", "create-alias", "--alias-name", "alias/larder-app", "--target-key-id", app]);
  cli(["kms", "create-alias", "--alias-name", "alias/larder-queues", "--target-key-id", app]);
  cli(["kms", "rotate-key-on-demand", "--key-id", app]);
  const legacy = key(["--description", "The key the queues used before larder-app"]);
  cli(["kms", "create-alias", "--alias-name", "alias/larder-legacy", "--target-key-id", legacy]);
  cli(["kms", "disable-key", "--key-id", legacy]);
  const signing = key(["--key-spec", "RSA_2048", "--key-usage", "SIGN_VERIFY", "--description", "Signs invoice PDFs"]);
  cli(["kms", "create-alias", "--alias-name", "alias/larder-signing", "--target-key-id", signing]);
  const old = key(["--key-spec", "ECC_NIST_P256", "--key-usage", "SIGN_VERIFY", "--description", "The previous signing key"]);
  cli(["kms", "create-alias", "--alias-name", "alias/larder-old-signing", "--target-key-id", old]);
  cli(["kms", "schedule-key-deletion", "--key-id", old, "--pending-window-in-days", "7"]);
  key([]);

  // The queue that uses the first key.
  cli(["sqs", "create-queue", "--queue-name", "pick-lists", "--attributes", JSON.stringify({ KmsMasterKeyId: "alias/larder-app", KmsDataKeyReusePeriodSeconds: "300" })]);

  // Secrets: stripe-key (two versions, rotated, tagged, with a resource policy) and db-credentials.
  const role = `arn:aws:iam::${account}:role/lambda-exec`;
  cli(["lambda", "create-function", "--function-name", "rotate-stripe-key", "--runtime", "python3.13", "--role", role, "--handler", "lambda_function.lambda_handler", "--zip-file", `fileb://${zipOf("lambda_function.py", ROTATE)}`, "--timeout", "30"]);
  const stripe = "larder/prod/stripe-key";
  cli(["secretsmanager", "create-secret", "--name", stripe, "--description", "Stripe live API key and webhook signing secret used by payments.", "--secret-string", '{"secret_key":"sk_live_EXAMPLE_old","webhook_secret":"whsec_EXAMPLE_old"}', "--kms-key-id", "alias/larder-app", "--tags", "Key=app,Value=payments", "Key=env,Value=prod", "Key=team,Value=payments"]);
  cli(["secretsmanager", "put-secret-value", "--secret-id", stripe, "--secret-string", STRIPE]);
  cli(["secretsmanager", "put-resource-policy", "--secret-id", stripe, "--resource-policy", secretPolicy(account)]);
  // Rotated once by the function: the rotation runs after RotateSecret answers, so wait for the day it finished.
  cli(["secretsmanager", "rotate-secret", "--secret-id", stripe, "--rotation-lambda-arn", `arn:aws:lambda:us-east-1:${account}:function:rotate-stripe-key`, "--rotation-rules", '{"ScheduleExpression":"rate(30 days)","Duration":"3h"}']);
  for (let i = 0; i < 60 && !cli(["secretsmanager", "describe-secret", "--secret-id", stripe]).includes("LastRotatedDate"); i++) Bun.sleepSync(250);
  cli(["secretsmanager", "create-secret", "--name", "larder/prod/db-credentials", "--description", "Postgres credentials for the larder application database.", "--secret-string", '{"username":"larder","password":"hunter2-EXAMPLE"}']);

  // Parameters.
  const put = (name: string, type: string, value: string, description: string, extra: string[] = []) => cli(["ssm", "put-parameter", "--name", name, "--type", type, "--value", value, "--description", description, ...extra]);
  put("/larder/prod/api/allowed-origins", "StringList", "https://larder.dev,https://app.larder.dev", "Origins the public API accepts");
  put("/larder/prod/db/host", "String", "db-old.larder.internal", "Primary database host");
  put("/larder/prod/db/host", "String", "db.larder.internal", "Primary database host", ["--overwrite"]);
  put("/larder/prod/db/password", "SecureString", "first-EXAMPLE-pass", "Database password for the prod stack", ["--key-id", "alias/larder-app", "--tags", "Key=app,Value=larder", "Key=env,Value=prod", "Key=cost-center,Value=fulfilment"]);
  put("/larder/prod/db/password", "SecureString", "second-EXAMPLE-pass", "Database password for the prod stack", ["--overwrite"]);
  put("/larder/prod/db/password", "SecureString", "kq7Vz-2mPf-Lr9Xw-Tn40", "Database password for the prod stack", ["--overwrite"]);
  cli(["ssm", "label-parameter-version", "--name", "/larder/prod/db/password", "--parameter-version", "2", "--labels", "current"]);
  cli(["ssm", "label-parameter-version", "--name", "/larder/prod/db/password", "--parameter-version", "3", "--labels", "canary"]);
  put("/larder/prod/feature/new-checkout", "String", "off", "Turns the new checkout flow on");
  put("/larder/prod/queues/pick-lists", "String", "http://sqs.us-east-1.larder.internal/pick-lists", "URL of the pick-lists queue");
  put("/larder/prod/search/synonyms", "String", "tote=bag,crate=box", "Search synonym list (6 KB)", ["--tier", "Advanced"]);
  put("/larder/staging/db/host", "String", "db.staging.larder.internal", "Staging database host");
  put("/larder/staging/feature/new-checkout", "String", "off", "Staging checkout flag");
  put("/larder/staging/feature/new-checkout", "String", "on", "Staging checkout flag", ["--overwrite"]);
}

if (import.meta.main) {
  const [endpoint, account] = process.argv.slice(2);
  if (!endpoint) throw new Error("usage: bun e2e/tests/kmsetc-seed.ts <endpoint> [account]");
  seedKmsetc(endpoint, account ?? "111122223333");
  console.log("seeded");
}
