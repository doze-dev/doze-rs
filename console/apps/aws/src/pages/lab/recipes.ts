// Recipes: a call to try with the person's OWN thing, started from a row of one of the Resources lists (the first queue, the first table).
// A recipe exists only for a thing that exists. Pure: `recipes-panel.tsx` finds the rows and draws the cards.
import type { ColumnSpec } from "../../api/types/ColumnSpec";
import type { ResourceRow } from "../../api/types/ResourceRow";
import type { Ask } from "./history";

/** Where "what to look at next" goes: the resource's own page, or Live filtered to the service. */
export type Next = { kind: "resource"; service: string; resKind: string; name: string; view?: string } | { kind: "live"; service: string };

export type Recipe = {
  /** The recipe's own name (`sqs-send`). */
  id: string;
  /** The service as Resources names it, for the Live link and the card's tag. */
  service: string;
  title: string;
  /** What the call is, in one line. */
  line: string;
  ask: Omit<Ask, "title">;
  /** What a fault rule names to bend the calls for this thing alone (a pattern the gateway's resource fits), when the call names one. */
  resource?: string;
  /** The call's answer carries a secret: it is hidden until the person shows it. */
  secret: boolean;
  next: { text: string; to: Next };
};

export type RecipeSpec = {
  id: string;
  /** The service and kind whose list the first row comes from. */
  service: string;
  kind: string;
  build: (row: ResourceRow, columns: readonly ColumnSpec[]) => Recipe | null;
};

const cell = (columns: readonly ColumnSpec[], row: ResourceRow, title: string): string | null => {
  const i = columns.findIndex((c) => c.title === title);
  return i >= 0 ? (row.cells[i] ?? null) : null;
};

const HELLO = "Hello from the Lab";
const b64 = (s: string) => btoa(s);

/** A DynamoDB key attribute's value, by the type the table declares for it (`S`, `N` or `B`): a string by default. */
function keyValue(type: string | undefined, which: string): Record<string, string> {
  if (type === "N") return { N: "1" };
  if (type === "B") return { B: b64(`lab-${which}`) };
  return { S: `lab-${which}` };
}

/** The attribute types of a table, from its `Attributes` cell (`[{"AttributeName":"id","AttributeType":"S"}]`). */
function attributeTypes(text: string | null): Record<string, string> {
  try {
    const v: unknown = JSON.parse(text ?? "[]");
    const out: Record<string, string> = {};
    for (const a of Array.isArray(v) ? (v as Record<string, unknown>[]) : []) if (typeof a.AttributeName === "string" && typeof a.AttributeType === "string") out[a.AttributeName] = a.AttributeType;
    return out;
  } catch {
    return {};
  }
}

const sqs: RecipeSpec = {
  id: "sqs-send",
  service: "sqs",
  kind: "queues",
  build: (row) => {
    const fifo = row.name.endsWith(".fifo");
    return {
      id: "sqs-send",
      service: "sqs",
      title: `Send a message to ${row.name}`,
      line: "SQS SendMessage with the queue's URL filled in.",
      resource: row.name,
      ask: { target: "sqs", service: "sqs", op: "SendMessage", input: { QueueUrl: row.id, MessageBody: HELLO, ...(fifo ? { MessageGroupId: "lab", MessageDeduplicationId: `lab-${row.name}` } : {}) } },
      secret: false,
      next: { text: "Then open the queue and look at its messages.", to: { kind: "resource", service: "sqs", resKind: "queues", name: row.name, view: "messages" } },
    };
  },
};

const dynamodb: RecipeSpec = {
  id: "dynamodb-put",
  service: "dynamodb",
  kind: "tables",
  build: (row, columns) => {
    const partition = cell(columns, row, "Partition key");
    if (!partition) return null;
    const sort = cell(columns, row, "Sort key");
    const types = attributeTypes(cell(columns, row, "Attributes"));
    const item: Record<string, unknown> = { [partition]: keyValue(types[partition], "1") };
    if (sort) item[sort] = keyValue(types[sort], "1");
    return {
      id: "dynamodb-put",
      service: "dynamodb",
      title: `Put an item in ${row.name}`,
      line: `DynamoDB PutItem with its key (${sort ? `${partition} and ${sort}` : partition}) filled in.`,
      resource: row.name,
      ask: { target: "dynamodb", service: "dynamodb", op: "PutItem", input: { TableName: row.id, Item: item } },
      secret: false,
      next: { text: "Then open the table and look for the item.", to: { kind: "resource", service: "dynamodb", resKind: "tables", name: row.name } },
    };
  },
};

const lambda: RecipeSpec = {
  id: "lambda-invoke",
  service: "lambda",
  kind: "functions",
  build: (row) => ({
    id: "lambda-invoke",
    service: "lambda",
    title: `Invoke ${row.name}`,
    line: "Lambda Invoke with a small JSON payload (a blob is base64 text).",
    resource: row.name,
    ask: { target: "lambda", service: "lambda", op: "Invoke", input: { FunctionName: row.id, InvocationType: "RequestResponse", Payload: b64('{"source":"lab"}') } },
    secret: false,
    next: { text: "Then see the invocation on Live, and the function's log lines.", to: { kind: "live", service: "lambda" } },
  }),
};

const sns: RecipeSpec = {
  id: "sns-publish",
  service: "sns",
  kind: "topics",
  build: (row) => ({
    id: "sns-publish",
    service: "sns",
    title: `Publish to ${row.name}`,
    line: "SNS Publish with the topic's ARN filled in.",
    resource: `*:${row.name}`,
    ask: { target: "sns", service: "sns", op: "Publish", input: { TopicArn: row.id, Message: HELLO, ...(row.name.endsWith(".fifo") ? { MessageGroupId: "lab", MessageDeduplicationId: `lab-${row.name}` } : {}) } },
    secret: false,
    next: { text: "Then open the topic and follow what it delivered.", to: { kind: "resource", service: "sns", resKind: "topics", name: row.name } },
  }),
};

// An object cannot be put from the Lab: the Lab sends over the bus, which carries no object body. So the bucket's recipe looks inside it.
const s3: RecipeSpec = {
  id: "s3-list",
  service: "s3",
  kind: "buckets",
  build: (row) => ({
    id: "s3-list",
    service: "s3",
    title: `Look inside ${row.name}`,
    line: "S3 ListObjectsV2. (PutObject cannot be sent from here: upload on the bucket's page.)",
    resource: row.name,
    ask: { target: "s3", service: "s3", op: "ListObjectsV2", input: { Bucket: row.id } },
    secret: false,
    next: { text: "Then open the bucket to upload an object.", to: { kind: "resource", service: "s3", resKind: "buckets", name: row.name } },
  }),
};

const states: RecipeSpec = {
  id: "states-start",
  service: "stepfunctions",
  kind: "machines",
  build: (row) => ({
    id: "states-start",
    service: "stepfunctions",
    title: `Start ${row.name}`,
    line: "Step Functions StartExecution with the state machine's ARN filled in.",
    resource: `*:${row.name}`,
    ask: { target: "states", service: "stepfunctions", op: "StartExecution", input: { stateMachineArn: row.id, input: "{}" } },
    secret: false,
    next: { text: "Then open the state machine and watch the execution.", to: { kind: "resource", service: "stepfunctions", resKind: "machines", name: row.name } },
  }),
};

const secrets: RecipeSpec = {
  id: "secrets-get",
  service: "secretsmanager",
  kind: "secrets",
  build: (row) => ({
    id: "secrets-get",
    service: "secretsmanager",
    title: `Get ${row.name}`,
    line: "Secrets Manager GetSecretValue with the secret's name filled in.",
    resource: row.name,
    ask: { target: "secretsmanager", service: "secretsmanager", op: "GetSecretValue", input: { SecretId: row.name } },
    secret: true,
    next: { text: "Then see the call on Live.", to: { kind: "live", service: "secretsmanager" } },
  }),
};

const events: RecipeSpec = {
  id: "events-put",
  service: "eventbridge",
  kind: "buses",
  build: (row) => ({
    id: "events-put",
    service: "eventbridge",
    title: `Put an event on ${row.name}`,
    line: "EventBridge PutEvents with the bus's name filled in.",
    ask: { target: "events", service: "eventbridge", op: "PutEvents", input: { Entries: [{ EventBusName: row.id, Source: "doze.lab", DetailType: "Lab test event", Detail: JSON.stringify({ hello: "lab" }) }] } },
    secret: false,
    next: { text: "Then open the bus and see which rules took it.", to: { kind: "resource", service: "eventbridge", resKind: "buses", name: row.name } },
  }),
};

/** The recipes, in the order their cards are drawn. */
export const SPECS: readonly RecipeSpec[] = [sqs, dynamodb, lambda, sns, s3, states, secrets, events];

/** A secret's value in an answer's text, hidden: the `SecretString` and `SecretBinary` members are replaced by a mark of the same shape. */
export function hideSecret(output: string): string {
  return output.replace(/("(?:SecretString|SecretBinary)":\s*)"(?:[^"\\]|\\.)*"/g, '$1"••••••••"');
}
