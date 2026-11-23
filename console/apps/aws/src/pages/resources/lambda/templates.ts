// The example events of the Test tab: the shapes the services that invoke a function send it, filled with this function's own
// triggers where it has them. Pure.
import type { Trigger } from "./model";

export type Template = { id: string; label: string; event: unknown; /** Which trigger it is the shape of, by the trigger's kind. */ of?: string };

type Where = { region: string; account: string };

const sns = (w: Where, topic: string) => ({ Records: [{ EventSource: "aws:sns", EventVersion: "1.0", EventSubscriptionArn: `arn:aws:sns:${w.region}:${w.account}:${topic}:4f6c1b7e-2a0d-4a56-9b0e-6d3f8a1c5e92`, Sns: { Type: "Notification", MessageId: "5b8e0c2d-71a4-5f0e-9c3a-0e6b1d4f7a28", TopicArn: `arn:aws:sns:${w.region}:${w.account}:${topic}`, Subject: "example subject", Message: "example message", Timestamp: "2026-01-01T00:00:00.000Z" } }] });
const sqs = (w: Where, queue: string) => ({ Records: [{ messageId: "059f36b4-87a3-44ab-83d2-661975830a7d", receiptHandle: "AQEBwJnKyrHigUMZj6rYigCgxlaS3SLy0a", body: "example message", attributes: { ApproximateReceiveCount: "1", SentTimestamp: "1767225600000", SenderId: w.account, ApproximateFirstReceiveTimestamp: "1767225600001" }, messageAttributes: {}, md5OfBody: "e4e68fb7bd0e697a0ae8f1bb342846b3", eventSource: "aws:sqs", eventSourceARN: `arn:aws:sqs:${w.region}:${w.account}:${queue}`, awsRegion: w.region }] });
const s3 = (w: Where, bucket: string) => ({ Records: [{ eventVersion: "2.1", eventSource: "aws:s3", awsRegion: w.region, eventTime: "2026-01-01T00:00:00.000Z", eventName: "ObjectCreated:Put", s3: { s3SchemaVersion: "1.0", bucket: { name: bucket, arn: `arn:aws:s3:::${bucket}` }, object: { key: "example.txt", size: 1024, eTag: "0123456789abcdef0123456789abcdef" } } }] });
const scheduled = (w: Where) => ({ version: "0", id: "53dc4d37-cffa-4f76-80c9-8b7d4a4d2eaa", "detail-type": "Scheduled Event", source: "aws.events", account: w.account, time: "2026-01-01T00:00:00Z", region: w.region, resources: [`arn:aws:events:${w.region}:${w.account}:rule/example`], detail: {} });
const http = () => ({ version: "2.0", routeKey: "$default", rawPath: "/", rawQueryString: "", headers: { "content-type": "application/json" }, requestContext: { http: { method: "POST", path: "/" } }, body: '{"key":"value"}', isBase64Encoded: false });

/** The templates offered for a function with these triggers: the shapes of its own triggers first, then the common ones. */
export function templatesFor(triggers: readonly Trigger[], where: Where): Template[] {
  const own: Template[] = [];
  for (const t of triggers) {
    if (t.kind === "SNS topic") own.push({ id: `sns-${t.name}`, label: `SNS topic ${t.name}`, event: sns(where, t.name), of: t.kind });
    if (t.kind === "SQS queue") own.push({ id: `sqs-${t.name}`, label: `SQS queue ${t.name}`, event: sqs(where, t.name), of: t.kind });
    if (t.kind === "S3 bucket") own.push({ id: `s3-${t.name}`, label: `S3 bucket ${t.name}`, event: s3(where, t.name), of: t.kind });
    if (t.kind === "EventBridge rule") own.push({ id: `events-${t.name}`, label: `EventBridge rule ${t.name.split("/").pop()}`, event: scheduled(where), of: t.kind });
  }
  const seen = new Set(own.map((t) => t.id));
  const common: Template[] = [
    { id: "hello", label: "Hello world", event: { key1: "value1", key2: "value2", key3: "value3" } },
    { id: "sns", label: "SNS topic notification", event: sns(where, "example-topic") },
    { id: "sqs", label: "SQS message", event: sqs(where, "example-queue") },
    { id: "s3", label: "S3 put", event: s3(where, "example-bucket") },
    { id: "scheduled", label: "EventBridge scheduled event", event: scheduled(where) },
    { id: "http", label: "Function URL request", event: http() },
  ];
  return [...own, ...common.filter((t) => !seen.has(t.id))];
}

/** The event a Test tab starts with: its first trigger's shape, else a small object. */
export function firstEvent(templates: readonly Template[]): { id: string; text: string } {
  const t = templates.find((x) => x.of) ?? templates.find((x) => x.id === "hello");
  return { id: t?.id ?? "hello", text: JSON.stringify(t?.event ?? { key: "value" }, null, 2) };
}
