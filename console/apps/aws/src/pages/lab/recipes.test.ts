import { describe, expect, test } from "bun:test";
import type { ColumnSpec } from "../../api/types/ColumnSpec";
import type { ResourceRow } from "../../api/types/ResourceRow";
import { hideSecret, SPECS } from "./recipes";

const spec = (id: string) => {
  const s = SPECS.find((x) => x.id === id);
  if (!s) throw new Error(`no recipe ${id}`);
  return s;
};
const row = (name: string, id = name, cells: (string | null)[] = []): ResourceRow => ({ name, label: name, id, cells, link: null });
const col = (title: string): ColumnSpec => ({ title, format: "text", badge: null });

describe("recipes", () => {
  test("a queue's recipe sends to its URL, and a FIFO queue's carries the group and de-duplication ids it needs", () => {
    const r = spec("sqs-send").build(row("orders", "http://h/111122223333/orders"), []);
    expect(r?.ask).toMatchObject({ target: "sqs", op: "SendMessage", input: { QueueUrl: "http://h/111122223333/orders", MessageBody: "Hello from the Lab" } });
    expect(r?.title).toBe("Send a message to orders");
    const f = spec("sqs-send").build(row("orders.fifo", "http://h/1/orders.fifo"), []);
    expect(f?.ask.input).toMatchObject({ MessageGroupId: "lab", MessageDeduplicationId: "lab-orders.fifo" });
    expect(r?.ask.input).not.toHaveProperty("MessageGroupId");
  });

  test("a table's recipe builds an item from its key, in the type the table declares", () => {
    const columns = [col("Status"), col("Partition key"), col("Sort key"), col("Attributes")];
    const attrs = JSON.stringify([{ AttributeName: "id", AttributeType: "S" }, { AttributeName: "n", AttributeType: "N" }]);
    const r = spec("dynamodb-put").build(row("orders", "orders", ["ACTIVE", "id", "n", attrs]), columns);
    expect(r?.ask.input).toEqual({ TableName: "orders", Item: { id: { S: "lab-1" }, n: { N: "1" } } });
    const b = spec("dynamodb-put").build(row("t", "t", ["ACTIVE", "k", null, JSON.stringify([{ AttributeName: "k", AttributeType: "B" }])]), columns);
    expect(b?.ask.input).toEqual({ TableName: "t", Item: { k: { B: btoa("lab-1") } } });
    // No key to be read: no recipe, rather than a guess.
    expect(spec("dynamodb-put").build(row("t", "t", ["ACTIVE", null, null, null]), columns)).toBeNull();
  });

  test("a function's payload is base64 text, as the model says a blob is", () => {
    const r = spec("lambda-invoke").build(row("resize"), []);
    const input = r?.ask.input as { Payload: string };
    expect(atob(input.Payload)).toBe('{"source":"lab"}');
  });

  test("a bucket has two recipes: look inside it, and put a short text note in it, whose body is the input's Body", () => {
    const r = spec("s3-list").build(row("images"), []);
    expect(r?.ask).toMatchObject({ op: "ListObjectsV2", input: { Bucket: "images" } });
    const p = spec("s3-put").build(row("images"), []);
    expect(p?.ask).toMatchObject({ op: "PutObject", input: { Bucket: "images", Key: "lab-note.txt", Body: "Hello from the Lab" } });
    expect(p?.resource).toBe("images/lab-note.txt");
  });

  test("a state machine, a topic, a bus and a secret name themselves in the call", () => {
    expect(spec("states-start").build(row("flow", "arn:aws:states:us-east-1:1:stateMachine:flow"), [])?.ask).toMatchObject({ target: "states", service: "stepfunctions", input: { stateMachineArn: "arn:aws:states:us-east-1:1:stateMachine:flow" } });
    expect(spec("sns-publish").build(row("t", "arn:aws:sns:us-east-1:1:t"), [])?.ask.input).toMatchObject({ TopicArn: "arn:aws:sns:us-east-1:1:t" });
    const e = spec("events-put").build(row("default"), []);
    expect(e?.ask.input).toMatchObject({ Entries: [{ EventBusName: "default", Source: "doze.lab" }] });
    const s = spec("secrets-get").build(row("db-password", "arn:aws:secretsmanager:us-east-1:1:secret:db-password-AbCdEf"), []);
    expect(s?.secret).toBe(true);
    expect(s?.ask.input).toEqual({ SecretId: "db-password" });
  });

  test("every recipe has its own id, a kind of Resources to find its row in, and a next step", () => {
    expect(new Set(SPECS.map((s) => s.id)).size).toBe(SPECS.length);
    for (const s of SPECS) {
      const r = s.build(row("x", "x", ["a", "id", null, "[]"]), [col("Status"), col("Partition key"), col("Sort key"), col("Attributes")]);
      expect(r?.id).toBe(s.id);
      expect(r?.next.text.length).toBeGreaterThan(10);
      expect(r?.service).toBe(s.service);
    }
  });

  test("a secret's value is hidden in an answer, and nothing else is", () => {
    const out = JSON.stringify({ Name: "db", SecretString: 'p"w\\d', SecretBinary: "AAEC", VersionStages: ["AWSCURRENT"] }, null, 2);
    const hidden = hideSecret(out);
    expect(hidden).not.toContain("AAEC");
    expect(hidden).not.toContain("w\\\\d");
    expect(hidden).toContain('"Name": "db"');
    expect(hidden).toContain("AWSCURRENT");
  });
});
