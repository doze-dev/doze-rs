// The world the Step Functions boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: a role Step Functions
// and Lambda may assume, two queues, an orders table, the send-email function (it reads event["email"]), two activities, the Standard
// machine order-fulfilment (three versions, the aliases PROD and STAGING, eight executions: running, succeeded, failed, aborted) and
// the Express machine thumbnail-batch with nothing at all. `bun e2e/tests/stepfunctions-seed.ts http://127.0.0.1:45800 [account]` seeds a
// running one by hand.
import { aws } from "../server";
import { zipOf } from "./lambda-seed";

type Cli = (args: string[]) => string;

function runner(endpoint: string): Cli {
  return (args) => {
    const r = aws(endpoint, args);
    if (r.code !== 0) throw new Error(`aws ${args.join(" ")}\n${r.out}`);
    return r.out;
  };
}

const SEND_EMAIL = `def lambda_handler(event, context):\n    return {"sent": event["email"]}\n`;

/** The machine, in the version the board's Definition tab shows (the heartbeat is the one that changes between versions). */
export function orderDefinition(queueUrl: string, account: string, heartbeat: number, withActivity = true): string {
  const states: Record<string, unknown> = {
    LoadOrder: {
      Type: "Task",
      Resource: "arn:aws:states:::dynamodb:getItem",
      Parameters: { TableName: "orders-table", Key: { order_id: { "S.$": "$.order_id" }, created_at: { "S.$": "$.created_at" } } },
      ResultPath: "$.order",
      Next: "CreatePickList",
    },
    CreatePickList: {
      Type: "Task",
      Resource: "arn:aws:states:::sqs:sendMessage",
      Parameters: { QueueUrl: queueUrl, "MessageBody.$": "States.JsonToString($.order.Item)" },
      ResultPath: null,
      Next: withActivity ? "PackOrder" : "NotifyCustomer",
    },
    ...(withActivity ? { PackOrder: { Type: "Task", Resource: `arn:aws:states:us-east-1:${account}:activity:pack-order`, HeartbeatSeconds: heartbeat, ResultPath: "$.packed", Next: "NotifyCustomer" } } : {}),
    NotifyCustomer: {
      Type: "Task",
      Resource: "arn:aws:states:::lambda:invoke",
      Parameters: { FunctionName: "send-email", "Payload.$": "$" },
      Retry: [{ ErrorEquals: ["Lambda.ServiceException"], IntervalSeconds: 2, MaxAttempts: 3, BackoffRate: 2 }],
      End: true,
    },
  };
  return JSON.stringify({ Comment: "Order fulfilment for the larder stack", StartAt: "LoadOrder", TimeoutSeconds: 3600, States: states }, null, 2);
}

export type Larder = { machine: string; versions: string[]; running: string[]; failed: string; account: string };

const sleep = (ms: number) => Bun.sleepSync(ms);

export function seedLarder(endpoint: string, account: string): Larder {
  const cli = runner(endpoint);
  const json = <T>(args: string[]): T => JSON.parse(cli(args)) as T;
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: ["states.amazonaws.com", "lambda.amazonaws.com"] }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "larder-admin", "--assume-role-policy-document", trust]);
  const role = `arn:aws:iam::${account}:role/larder-admin`;
  const pick = json<{ QueueUrl: string }>(["sqs", "create-queue", "--queue-name", "pick-lists"]).QueueUrl;
  cli(["sqs", "create-queue", "--queue-name", "inventory-queue"]);
  cli(["dynamodb", "create-table", "--table-name", "orders-table", "--billing-mode", "PAY_PER_REQUEST", "--attribute-definitions", "AttributeName=order_id,AttributeType=S", "AttributeName=created_at,AttributeType=S", "--key-schema", "AttributeName=order_id,KeyType=HASH", "AttributeName=created_at,KeyType=RANGE"]);
  for (const id of ["ord-20261108-0412", "ord-20261108-0415", "ord-20261108-0416", "ord-20261108-0417", "ord-20261108-0418", "ord-20261108-0419", "ord-20261108-0420", "ord-20261107-1893"]) {
    cli(["dynamodb", "put-item", "--table-name", "orders-table", "--item", JSON.stringify({ order_id: { S: id }, created_at: { S: "2026-11-08T08:00:00Z" }, sku: { S: "oat-milk" } })]);
  }
  cli(["lambda", "create-function", "--function-name", "send-email", "--runtime", "python3.13", "--handler", "lambda_function.lambda_handler", "--role", role, "--zip-file", `fileb://${zipOf("lambda_function.py", SEND_EMAIL)}`]);
  for (const a of ["pack-order", "print-label"]) cli(["stepfunctions", "create-activity", "--name", a]);

  // Three versions of the machine: the first has no activity, the second adds it with a 60 second heartbeat, the third 120.
  const machine = "order-fulfilment";
  const created = json<{ stateMachineArn: string; stateMachineVersionArn: string }>(["stepfunctions", "create-state-machine", "--name", machine, "--role-arn", role, "--definition", orderDefinition(pick, account, 60, false), "--publish", "--version-description", "First release", "--tags", "key=app,value=orders", "key=env,value=dev", "key=stack,value=larder"]);
  const arn = created.stateMachineArn;
  const v2 = (() => {
    cli(["stepfunctions", "update-state-machine", "--state-machine-arn", arn, "--definition", orderDefinition(pick, account, 60)]);
    return json<{ stateMachineVersionArn: string }>(["stepfunctions", "publish-state-machine-version", "--state-machine-arn", arn, "--description", "Adds the pack-order activity"]).stateMachineVersionArn;
  })();
  cli(["stepfunctions", "update-state-machine", "--state-machine-arn", arn, "--definition", orderDefinition(pick, account, 120)]);
  const v3 = json<{ stateMachineVersionArn: string }>(["stepfunctions", "publish-state-machine-version", "--state-machine-arn", arn, "--description", "Heartbeat on pack-order"]).stateMachineVersionArn;
  const alias = (name: string, description: string, version: string) =>
    json<{ stateMachineAliasArn: string }>(["stepfunctions", "create-state-machine-alias", "--name", name, "--description", description, "--routing-configuration", JSON.stringify([{ stateMachineVersionArn: version, weight: 100 }])]).stateMachineAliasArn;
  const prod = alias("PROD", "Production traffic", v2);
  alias("STAGING", "Next version, before it goes live", v3);

  cli(["stepfunctions", "create-state-machine", "--name", "thumbnail-batch", "--type", "EXPRESS", "--role-arn", role, "--definition", JSON.stringify({ StartAt: "Resize", States: { Resize: { Type: "Pass", End: true } } })]);

  const activity = `arn:aws:states:us-east-1:${account}:activity:pack-order`;
  const start = (target: string, name: string, email: boolean) =>
    json<{ executionArn: string }>(["stepfunctions", "start-execution", "--state-machine-arn", target, "--name", name, "--input", JSON.stringify({ order_id: name.startsWith("ord-20261107") ? "ord-20261107-1893" : name, created_at: "2026-11-08T08:00:00Z", ...(email ? { email: "priya@example.com" } : {}) })]).executionArn;
  const status = (e: string) => json<{ status: string }>(["stepfunctions", "describe-execution", "--execution-arn", e]).status;
  const settle = (e: string, want: string) => {
    for (let i = 0; i < 100; i++) {
      if (status(e) === want) return;
      sleep(150);
    }
    throw new Error(`${e} did not become ${want}`);
  };
  // A worker takes the PackOrder task of the machine's newest waiting execution and answers it.
  const work = () => {
    for (let i = 0; i < 40; i++) {
      const task = json<{ taskToken?: string }>(["stepfunctions", "get-activity-task", "--activity-arn", activity, "--worker-name", "fulfilment"]);
      if (task.taskToken) {
        cli(["stepfunctions", "send-task-success", "--task-token", task.taskToken, "--task-output", "{}"]);
        return;
      }
    }
    throw new Error("no task for pack-order");
  };
  const finished: [string, string, boolean, string][] = [
    [prod, "ord-20261108-0419", true, "SUCCEEDED"],
    [prod, "ord-20261108-0420", true, "SUCCEEDED"],
    [prod, "ord-20261108-0417", false, "FAILED"],
    [arn, "ord-20261108-0416", true, "SUCCEEDED"],
  ];
  for (const [target, name, email, want] of finished) {
    const e = start(target, name, email);
    waitForTask(cli, activity);
    work();
    settle(e, want);
  }
  const failed = `arn:aws:states:us-east-1:${account}:execution:${machine}:ord-20261108-0417`;
  // One is stopped while it waits for a worker; two are left waiting.
  const aborted = start(prod, "ord-20261107-1893", true);
  waitForTask(cli, activity, false);
  cli(["stepfunctions", "stop-execution", "--execution-arn", aborted, "--cause", "stopped by hand"]);
  const running = [start(v3, "ord-20261108-0418", true), start(prod, "ord-20261108-0415", true)];
  return { machine, versions: [created.stateMachineVersionArn, v2, v3], running, failed, account };
}

/** Waits until an execution has scheduled a task for the activity (a worker's poll would then be answered at once). With `leave`
 * false the task is left where it is (the poll that finds it is not made). */
function waitForTask(cli: Cli, activity: string, _take = true): void {
  for (let i = 0; i < 100; i++) {
    const out = JSON.parse(cli(["stepfunctions", "list-executions", "--state-machine-arn", activity.replace(":activity:pack-order", ":stateMachine:order-fulfilment"), "--status-filter", "RUNNING"])) as { executions: { executionArn: string }[] };
    for (const e of out.executions) {
      const h = JSON.parse(cli(["stepfunctions", "get-execution-history", "--execution-arn", e.executionArn, "--max-results", "50"])) as { events: { type: string }[] };
      if (h.events.some((x) => x.type === "ActivityScheduled") && !h.events.some((x) => x.type === "ActivityStarted")) return;
    }
    sleep(120);
  }
  throw new Error("no execution reached PackOrder");
}

if (import.meta.main) {
  const [endpoint, account] = process.argv.slice(2);
  if (!endpoint) throw new Error("usage: bun stepfunctions-seed.ts <endpoint> [account]");
  console.log(JSON.stringify(seedLarder(endpoint, account ?? "111122223333")));
}
