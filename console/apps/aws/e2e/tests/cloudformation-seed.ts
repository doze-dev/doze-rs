// The world the CloudFormation boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: a topic and a role the stacks
// name, a bucket of templates, a table that clashes with the template the wizard makes, and eight stacks in every state the boards show:
// larder-core (parameters, outputs, exports, tags, a topic, a bucket it retains, two change sets), larder-edge (imports an export of
// larder-observability, and has a nested stack), larder-sandbox (no outputs, no parameters, no change sets), larder-hotfix (an update
// that rolled back), larder-nightly (a create that rolled back), larder-reports-job (waiting for its change set).
// `bun e2e/tests/cloudformation-seed.ts http://127.0.0.1:45590` seeds a running one by hand.
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aws } from "../server";

export type Cli = (args: string[]) => string;

export function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

export const CORE = `AWSTemplateFormatVersion: "2010-09-09"
Description: "Larder core: queues, topic, orders table, product images, resize function"
Parameters:
  Stage: { Type: String, Default: prod }
  VisibilityTimeout: { Type: Number, Default: 30 }
  MaxReceiveCount: { Type: Number, Default: 5 }
  ExecutionRoleArn: { Type: String }
  WebhookSecret: { Type: String, NoEcho: true }
Resources:
  PickListsDlq:
    Type: AWS::SQS::Queue
    Properties: { QueueName: pick-lists-dlq, MessageRetentionPeriod: 1209600 }
  PickListsQueue:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: pick-lists
      VisibilityTimeout: !Ref VisibilityTimeout
      RedrivePolicy:
        deadLetterTargetArn: !GetAtt PickListsDlq.Arn
        maxReceiveCount: !Ref MaxReceiveCount
  OrdersCreatedTopic:
    Type: AWS::SNS::Topic
    Properties: { TopicName: orders-created }
  OrdersTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: orders-table
      BillingMode: PAY_PER_REQUEST
      AttributeDefinitions: [ { AttributeName: id, AttributeType: S } ]
      KeySchema: [ { AttributeName: id, KeyType: HASH } ]
  ProductImagesBucket:
    Type: AWS::S3::Bucket
    DeletionPolicy: Retain
    Properties:
      BucketName: product-images
      VersioningConfiguration: { Status: Enabled }
  ResizeProductImage:
    Type: AWS::Lambda::Function
    Properties:
      FunctionName: resize-product-image
      Runtime: nodejs22.x
      Handler: index.handler
      Code: { ZipFile: "exports.handler = async () => ({})" }
      Role: !Ref ExecutionRoleArn
Outputs:
  OrdersTopicArn:
    Description: Topic that announces every new order
    Value: !Ref OrdersCreatedTopic
    Export: { Name: larder-core-OrdersTopicArn }
  PickListsQueueArn:
    Description: Queue the pickers read from
    Value: !GetAtt PickListsQueue.Arn
    Export: { Name: larder-core-PickListsQueueArn }
  PickListsQueueUrl:
    Description: URL for SendMessage and ReceiveMessage
    Value: !Ref PickListsQueue
`;

/** Version 2 of larder-core: a reports bucket, and the pick-lists queue keeps its messages for 7 days. */
export const CORE_V2 = CORE.replace("      VisibilityTimeout: !Ref VisibilityTimeout\n", "      VisibilityTimeout: !Ref VisibilityTimeout\n      MessageRetentionPeriod: 604800\n").replace("  ResizeProductImage:", "  ReportsBucket:\n    Type: AWS::S3::Bucket\n    Properties: { BucketName: larder-reports-out }\n  ResizeProductImage:");

export const SEARCH = `AWSTemplateFormatVersion: "2010-09-09"
Description: "Search index table and its reindex queue"
Parameters:
  Stage:
    Type: String
    Default: prod
    Description: Environment label added to the tables tags
  TableName:
    Type: String
    Default: search-index
    Description: Name of the DynamoDB table that holds the search index
  BillingMode:
    Type: String
    Default: PAY_PER_REQUEST
    Description: How the table is billed
    AllowedValues: [PAY_PER_REQUEST, PROVISIONED]
Resources:
  SearchIndexTable:
    Type: AWS::DynamoDB::Table
    Properties:
      TableName: !Ref TableName
      BillingMode: !Ref BillingMode
      AttributeDefinitions: [ { AttributeName: id, AttributeType: S } ]
      KeySchema: [ { AttributeName: id, KeyType: HASH } ]
  SearchReindexQueue:
    Type: AWS::SQS::Queue
    Properties: { QueueName: search-reindex }
`;

const json = (o: unknown) => JSON.stringify(o);
const OBSERVABILITY = json({ Resources: { DlqDepth: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/observability/dlq", Type: "String", Value: "pick-lists-dlq" } } }, Outputs: { DlqAlarmArn: { Value: { Ref: "DlqDepth" }, Export: { Name: "larder-observability-DlqAlarmArn" } } } });
const SANDBOX = json({ Resources: { Note: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/sandbox/note", Type: "String", Value: "scratch" } } } });
const API = json({ Resources: { ApiId: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/edge/api", Type: "String", Value: "r4w7l2k9zn" } } } });
const edge = (bucketUrl: string) => json({ Resources: {
  Imported: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/edge/alarm", Type: "String", Value: { "Fn::ImportValue": "larder-observability-DlqAlarmArn" } } },
  ApiStack: { Type: "AWS::CloudFormation::Stack", Properties: { TemplateURL: `${bucketUrl}/templates/api.json` } },
}, Outputs: { PublicApiId: { Value: { Ref: "Imported" }, Export: { Name: "larder-edge-PublicApiId" } } } });
const hotfix = (extra: boolean) => json({ Resources: {
  Flag: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/hotfix/flag", Type: "String", Value: "on" } },
  ...(extra ? { Taken: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/taken", Type: "String", Value: "x" } } } : {}),
} });
const NIGHTLY = json({ Resources: { Taken: { Type: "AWS::SSM::Parameter", Properties: { Name: "/larder/taken", Type: "String", Value: "y" } } } });
const REPORTS = json({ Resources: { Queue: { Type: "AWS::SQS::Queue", Properties: { QueueName: "reports-job-queue" } } } });

/** Waits until `stack` is no longer being worked on; its status. */
export async function settled(cli: Cli, stack: string, ms = 30_000): Promise<string> {
  const end = Date.now() + ms;
  for (;;) {
    let status: string;
    try {
      status = (JSON.parse(cli(["cloudformation", "describe-stacks", "--stack-name", stack])) as { Stacks: { StackStatus: string }[] }).Stacks[0]?.StackStatus ?? "";
    } catch {
      status = "";
    }
    if (status !== "" && !status.endsWith("_IN_PROGRESS")) return status;
    if (Date.now() > end) throw new Error(`${stack} did not settle (${status})`);
    await new Promise((r) => setTimeout(r, 150));
  }
}

/** Makes the larder world; the account is the one the doze-aws serves. */
export async function seedLarder(endpoint: string, account: string): Promise<void> {
  const cli = runner(endpoint);
  const dir = mkdtempSync(join(tmpdir(), "cfn-seed-"));
  const file = (name: string, text: string) => {
    const p = join(dir, name);
    writeFileSync(p, text);
    return `file://${p}`;
  };
  const stack = async (name: string, template: string, ...more: string[]) => {
    cli(["cloudformation", "create-stack", "--stack-name", name, "--template-body", file(`${name}.tpl`, template), ...more]);
    return settled(cli, name);
  };
  cli(["sns", "create-topic", "--name", "alerts"]);
  cli(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", json({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] })]);
  cli(["s3api", "create-bucket", "--bucket", "reports"]);
  for (const [key, body] of [["templates/larder-search.yaml", SEARCH], ["templates/larder-core-v2.yaml", CORE_V2], ["templates/api.json", API]] as const) {
    cli(["s3api", "put-object", "--bucket", "reports", "--key", key, "--body", file(key.replace("/", "-"), body).slice(7)]);
  }
  cli(["dynamodb", "create-table", "--table-name", "search-index", "--attribute-definitions", "AttributeName=id,AttributeType=S", "--key-schema", "AttributeName=id,KeyType=HASH", "--billing-mode", "PAY_PER_REQUEST"]);
  cli(["ssm", "put-parameter", "--name", "/larder/taken", "--type", "String", "--value", "made directly"]);
  const topic = `arn:aws:sns:us-east-1:${account}:alerts`;
  await stack("larder-observability", OBSERVABILITY);
  await stack("larder-core", CORE, "--parameters", `ParameterKey=ExecutionRoleArn,ParameterValue=arn:aws:iam::${account}:role/lambda-exec`, "ParameterKey=WebhookSecret,ParameterValue=hunter2", "--tags", "Key=app,Value=larder", "Key=stage,Value=prod", "--notification-arns", topic, "--capabilities", "CAPABILITY_IAM");
  await stack("larder-edge", edge(`https://reports.s3.us-east-1.amazonaws.com`));
  await stack("larder-sandbox", SANDBOX);
  await stack("larder-hotfix", hotfix(false));
  cli(["cloudformation", "update-stack", "--stack-name", "larder-hotfix", "--template-body", file("hotfix2.tpl", hotfix(true))]);
  await settled(cli, "larder-hotfix");
  await stack("larder-nightly", NIGHTLY);
  // Two change sets on larder-core: one that previews version 2, one that changes nothing.
  cli(["cloudformation", "create-change-set", "--stack-name", "larder-core", "--change-set-name", "add-reports-bucket", "--template-body", file("core2.tpl", CORE_V2), "--parameters", "ParameterKey=Stage,UsePreviousValue=true", "ParameterKey=VisibilityTimeout,UsePreviousValue=true", "ParameterKey=MaxReceiveCount,UsePreviousValue=true", "ParameterKey=ExecutionRoleArn,UsePreviousValue=true", "ParameterKey=WebhookSecret,UsePreviousValue=true", "--capabilities", "CAPABILITY_IAM"]);
  cli(["cloudformation", "create-change-set", "--stack-name", "larder-core", "--change-set-name", "same-as-deployed", "--use-previous-template", "--parameters", "ParameterKey=Stage,UsePreviousValue=true", "ParameterKey=VisibilityTimeout,UsePreviousValue=true", "ParameterKey=MaxReceiveCount,UsePreviousValue=true", "ParameterKey=ExecutionRoleArn,UsePreviousValue=true", "ParameterKey=WebhookSecret,UsePreviousValue=true", "--capabilities", "CAPABILITY_IAM"]);
  // A stack made from a change set that nobody has run.
  cli(["cloudformation", "create-change-set", "--stack-name", "larder-reports-job", "--change-set-name", "first-look", "--change-set-type", "CREATE", "--template-body", file("reports.tpl", REPORTS)]);
}

if (import.meta.main) {
  const endpoint = process.argv[2] ?? "";
  await seedLarder(endpoint, process.argv[3] ?? "111122223333");
  console.log("seeded", endpoint);
}
