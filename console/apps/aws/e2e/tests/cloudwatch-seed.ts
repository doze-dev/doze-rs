// The world the CloudWatch boards are drawn in ("larder"), made with the real AWS CLI against a doze-aws: a role, two functions, the
// alerts topic (delivering to send-email), the custom metrics of Larder/Orders and Larder/Queues, five alarms (one in ALARM with a
// history, two OK, one without data, one with no tags and no description) and two dashboards. `bun e2e/tests/cloudwatch-seed.ts
// http://127.0.0.1:46100 [account]` seeds a running one by hand.
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

const PLAIN = `def lambda_handler(event, context):\n    return {"ok": True}\n`;

/** The body of larder-ops, as the dashboard board draws it: two graphs, a number, an alarm status grid and a runbook. */
export function larderOps(region: string, account: string): string {
  const alarm = (n: string) => `arn:aws:cloudwatch:${region}:${account}:alarm:${n}`;
  return JSON.stringify({
    widgets: [
      { type: "metric", x: 0, y: 0, width: 12, height: 6, properties: { title: "Orders placed", view: "timeSeries", stat: "Sum", period: 300, region, metrics: [["Larder/Orders", "OrdersPlaced", "Environment", "prod"]] } },
      { type: "metric", x: 12, y: 0, width: 12, height: 6, properties: { title: "resize-product-image errors", stat: "Sum", period: 300, region, metrics: [["AWS/Lambda", "Errors", "FunctionName", "resize-product-image"]] } },
      { type: "metric", x: 0, y: 6, width: 8, height: 6, properties: { title: "DLQ depth", view: "singleValue", stat: "Maximum", period: 60, region, metrics: [["Larder/Queues", "DlqMessages", "QueueName", "pick-lists-dlq"]] } },
      { type: "alarm", x: 8, y: 6, width: 8, height: 6, properties: { title: "Alarm status", alarms: ["dlq-not-empty", "resize-errors", "orders-placed-low", "api-5xx-rate"].map(alarm) } },
      { type: "text", x: 16, y: 6, width: 8, height: 6, properties: { markdown: "# Runbook\n\n**Dead-letter queue not empty**\n\nOpen `pick-lists-dlq`, read the message, fix the cause, then redrive it to `pick-lists`.\n\nAlerts go to the `alerts` topic." } },
    ],
  });
}

export function seedLarderCloudwatch(endpoint: string, account: string, region = "us-east-1"): void {
  const cli = runner(endpoint);
  const trust = JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Principal: { Service: "lambda.amazonaws.com" }, Action: "sts:AssumeRole" }] });
  cli(["iam", "create-role", "--role-name", "lambda-exec", "--assume-role-policy-document", trust]);
  const role = `arn:aws:iam::${account}:role/lambda-exec`;
  for (const name of ["send-email", "resize-product-image"]) {
    cli(["lambda", "create-function", "--function-name", name, "--runtime", "python3.13", "--role", role, "--handler", "lambda_function.lambda_handler", "--zip-file", `fileb://${zipOf("lambda_function.py", PLAIN)}`]);
  }
  const alerts = (JSON.parse(cli(["sns", "create-topic", "--name", "alerts"])) as { TopicArn: string }).TopicArn;
  cli(["sns", "subscribe", "--topic-arn", alerts, "--protocol", "lambda", "--notification-endpoint", `arn:aws:lambda:${region}:${account}:function:send-email`]);

  // Custom metrics: a few points in the last hours.
  const now = Date.now();
  const put = (ns: string, metric: string, dims: string, values: number[]) => {
    const data = values.map((v, i) => ({ MetricName: metric, Value: v, Timestamp: new Date(now - (values.length - i) * 300_000).toISOString(), Dimensions: dims.split(",").map((d) => ({ Name: d.split("=")[0], Value: d.split("=")[1] })) }));
    cli(["cloudwatch", "put-metric-data", "--namespace", ns, "--metric-data", JSON.stringify(data)]);
  };
  put("Larder/Orders", "OrdersPlaced", "Environment=prod", [20, 22, 24, 23, 28, 30, 26, 30, 27, 25]);
  put("Larder/Orders", "OrdersFailed", "Environment=prod", [2, 3, 3, 3, 3, 4, 3, 3, 4, 2]);
  put("Larder/Queues", "DlqMessages", "QueueName=pick-lists-dlq", [0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

  const alarm = (name: string, args: string[]) => cli(["cloudwatch", "put-metric-alarm", "--alarm-name", name, ...args]);
  alarm("dlq-not-empty", ["--alarm-description", "The dead-letter queue pick-lists-dlq holds a message.", "--namespace", "Larder/Queues", "--metric-name", "DlqMessages", "--dimensions", "Name=QueueName,Value=pick-lists-dlq", "--statistic", "Maximum", "--period", "60", "--evaluation-periods", "1", "--datapoints-to-alarm", "1", "--threshold", "1", "--comparison-operator", "GreaterThanOrEqualToThreshold", "--treat-missing-data", "ignore", "--alarm-actions", alerts]);
  alarm("resize-errors", ["--namespace", "AWS/Lambda", "--metric-name", "Errors", "--dimensions", "Name=FunctionName,Value=resize-product-image", "--statistic", "Sum", "--period", "300", "--evaluation-periods", "3", "--datapoints-to-alarm", "2", "--threshold", "3", "--comparison-operator", "GreaterThanOrEqualToThreshold"]);
  alarm("orders-placed-low", ["--namespace", "Larder/Orders", "--metric-name", "OrdersPlaced", "--dimensions", "Name=Environment,Value=prod", "--statistic", "Sum", "--period", "300", "--evaluation-periods", "3", "--threshold", "5", "--comparison-operator", "LessThanThreshold"]);
  alarm("api-5xx-rate", ["--namespace", "AWS/ApiGateway", "--metric-name", "5XXError", "--dimensions", "Name=ApiName,Value=orders", "Name=Stage,Value=prod", "--statistic", "Sum", "--period", "300", "--evaluation-periods", "1", "--threshold", "5", "--comparison-operator", "GreaterThanThreshold"]);
  alarm("orders-error-rate", ["--namespace", "Larder/Orders", "--metric-name", "OrdersFailed", "--dimensions", "Name=Environment,Value=prod", "--statistic", "Sum", "--period", "300", "--evaluation-periods", "1", "--threshold", "10", "--comparison-operator", "GreaterThanThreshold"]);
  cli(["cloudwatch", "tag-resource", "--resource-arn", `arn:aws:cloudwatch:${region}:${account}:alarm:dlq-not-empty`, "--tags", "Key=app,Value=larder", "Key=env,Value=prod", "Key=team,Value=platform"]);
  // The history of dlq-not-empty: it went OK, then ALARM (which runs its action).
  for (const [name, state] of [["resize-errors", "OK"], ["orders-placed-low", "OK"], ["dlq-not-empty", "OK"], ["dlq-not-empty", "ALARM"]] as const) {
    cli(["cloudwatch", "set-alarm-state", "--alarm-name", name, "--state-value", state, "--state-reason", state === "ALARM" ? "Threshold Crossed: 1 out of the last 1 datapoints were greater than or equal to the threshold (1) (minimum 1 datapoint for OK -> ALARM transition)." : "Threshold Crossed: no datapoints were breaching"]);
  }
  cli(["cloudwatch", "put-dashboard", "--dashboard-name", "larder-ops", "--dashboard-body", larderOps(region, account)]);
  cli(["cloudwatch", "put-dashboard", "--dashboard-name", "larder-api", "--dashboard-body", JSON.stringify({ widgets: [{ type: "text", x: 0, y: 0, width: 24, height: 3, properties: { markdown: "# API\n\nThe orders API." } }] })]);
}

if (import.meta.main) {
  const [endpoint, account = "111122223333"] = process.argv.slice(2);
  if (!endpoint) throw new Error("usage: bun cloudwatch-seed.ts <endpoint> [account]");
  seedLarderCloudwatch(endpoint, account);
}
