#!/usr/bin/env bun
// Creates a small set of throwaway resources named doze-rec-td-* so teardown.ts can be tested. Costs cents; teardown removes them.
import { aws, assertRecorder, EXPECT_ACCOUNT } from "./lib.ts";
import { writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

console.log(`recorder: ${await assertRecorder()}`);
const step = async (name: string, f: () => Promise<unknown>) => {
  try { await f(); console.log(`  created  ${name}`); } catch (e) { console.log(`  FAILED   ${name}: ${(e as Error).message}`); }
};
const dir = mkdtempSync(join(tmpdir(), "doze-seed-"));
const body = join(dir, "obj.txt");
writeFileSync(body, "hello\n");

const q = "doze-rec-td-queue";
await step("sqs queue", () => aws(["sqs", "create-queue", "--queue-name", q]));
await step("sns topic", () => aws(["sns", "create-topic", "--name", "doze-rec-td-topic"]));
await step("ssm parameter", () => aws(["ssm", "put-parameter", "--name", "doze-rec-td-param", "--value", "x", "--type", "String"]));
await step("secret", () => aws(["secretsmanager", "create-secret", "--name", "doze-rec-td-secret", "--secret-string", "x"]));
await step("dynamodb table (deletion protection on)", async () => {
  await aws(["dynamodb", "create-table", "--table-name", "doze-rec-td-table", "--attribute-definitions", "AttributeName=pk,AttributeType=S", "--key-schema", "AttributeName=pk,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST", "--deletion-protection-enabled"]);
  await aws(["dynamodb", "wait", "table-exists", "--table-name", "doze-rec-td-table"]);
});
await step("log group", () => aws(["logs", "create-log-group", "--log-group-name", "doze-rec-td-logs"]));
await step("eventbridge rule + target", async () => {
  await aws(["events", "put-rule", "--name", "doze-rec-td-rule", "--schedule-expression", "rate(1 day)", "--state", "DISABLED"]);
  await aws(["events", "put-targets", "--rule", "doze-rec-td-rule", "--targets", `Id=t1,Arn=arn:aws:sqs:us-east-1:${EXPECT_ACCOUNT}:${q}`]);
});
await step("cloudwatch alarm", () => aws(["cloudwatch", "put-metric-alarm", "--alarm-name", "doze-rec-td-alarm", "--namespace", "DozeRec", "--metric-name", "m", "--statistic", "Sum", "--period", "300", "--evaluation-periods", "1", "--threshold", "1", "--comparison-operator", "GreaterThanThreshold"]));
await step("s3 bucket, versioned, 2 versions + delete marker", async () => {
  const b = `doze-rec-td-${EXPECT_ACCOUNT}`;
  await aws(["s3api", "create-bucket", "--bucket", b]);
  await aws(["s3api", "put-bucket-versioning", "--bucket", b, "--versioning-configuration", "Status=Enabled"]);
  await aws(["s3api", "put-object", "--bucket", b, "--key", "a.txt", "--body", body]);
  await aws(["s3api", "put-object", "--bucket", b, "--key", "a.txt", "--body", body]);
  await aws(["s3api", "put-object", "--bucket", b, "--key", "b.txt", "--body", body]);
  await aws(["s3api", "delete-object", "--bucket", b, "--key", "b.txt"]);
});
await step("iam role with inline policy", async () => {
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  await aws(["iam", "create-role", "--role-name", "doze-rec-td-role", "--assume-role-policy-document", trust]);
  await aws(["iam", "put-role-policy", "--role-name", "doze-rec-td-role", "--policy-name", "inline", "--policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: "logs:CreateLogGroup", Resource: "*" }] })]);
});
await step("iam user + group", async () => {
  await aws(["iam", "create-user", "--user-name", "doze-rec-td-user"]);
  await aws(["iam", "create-group", "--group-name", "doze-rec-td-group"]);
  await aws(["iam", "add-user-to-group", "--user-name", "doze-rec-td-user", "--group-name", "doze-rec-td-group"]);
});
await step("iam customer policy, attached to the role", async () => {
  const r = await aws(["iam", "create-policy", "--policy-name", "doze-rec-td-policy", "--policy-document", JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: "sqs:GetQueueUrl", Resource: "*" }] })]);
  await aws(["iam", "attach-role-policy", "--role-name", "doze-rec-td-role", "--policy-arn", r.Policy.Arn]);
});
await step("kms key (tagged) + alias", async () => {
  const k = await aws(["kms", "create-key", "--description", "doze-rec teardown test", "--tags", "TagKey=doze-rec,TagValue=true"]);
  await aws(["kms", "create-alias", "--alias-name", "alias/doze-rec-td-key", "--target-key-id", k.KeyMetadata.KeyId]);
});
await step("cloudformation stack (one queue)", async () => {
  const tpl = JSON.stringify({ Resources: { Q: { Type: "AWS::SQS::Queue", Properties: { QueueName: "doze-rec-td-cfn-queue" } } } });
  await aws(["cloudformation", "create-stack", "--stack-name", "doze-rec-td-stack", "--template-body", tpl]);
  await aws(["cloudformation", "wait", "stack-create-complete", "--stack-name", "doze-rec-td-stack"]);
});
await step("kinesis stream (1 shard)", async () => {
  await aws(["kinesis", "create-stream", "--stream-name", "doze-rec-td-stream", "--shard-count", "1"]);
  await aws(["kinesis", "wait", "stream-exists", "--stream-name", "doze-rec-td-stream"]);
});
await step("api gateway rest api", () => aws(["apigateway", "create-rest-api", "--name", "doze-rec-td-api"]));
console.log("\nseeded; now run: bun teardown.ts   (dry run), then bun teardown.ts --delete");
