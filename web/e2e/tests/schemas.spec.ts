// The Schemas pages (slice 10d) in Firefox, WebKit and Chromium against the real binary and the registry beside it: the list's
// filters in the address, registering with the live check and its fix, a subject's versions, the diff, the compatibility
// dialog, Try, the deletes, records decoded with their schemas, a schema-encoded Produce, the topic's Schema tab and Connect.
// Every name carries the browser's, so the three runs against one broker never meet.
import type { APIRequestContext, Page } from "@playwright/test";
import { Kafka, logLevel } from "kafkajs";
import { expect, overflowX, test } from "./fixtures";
import { apiGet, apiPost, kafkaAddress } from "./r4";

const AVRO_V1 = '{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"}]}';
const AVRO_V2_BAD = '{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"qty","type":"int"}]}';
const AVRO_NOTE = '{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"note","type":["null","string"],"default":null}]}';
const JSON_SCHEMA = '{"type":"object","properties":{"sku":{"type":"string"},"qty":{"type":"integer"}},"required":["sku"]}';
const PROTO = 'syntax = "proto3";\npackage shop;\nmessage Payment { string id = 1; int64 cents = 2; }\n';

/** The registry's REST address, as Connect names it (the seeded binary's, or a demo broker's). */
async function registryUrl(request: APIRequestContext): Promise<string> {
  const c = await apiGet(request, "/api/v1/connect");
  expect(c.registry, "the console has a registry").not.toBeNull();
  return c.registry.url as string;
}

async function registerRest(request: APIRequestContext, url: string, subject: string, schemaType: string, schema: string): Promise<number> {
  const res = await request.post(`${url}/subjects/${encodeURIComponent(subject)}/versions`, { headers: { "Content-Type": "application/vnd.schemaregistry.v1+json" }, data: JSON.stringify({ schemaType, schema }) });
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()).id as number;
}

const varint = (n: bigint): number[] => {
  const out: number[] = [];
  let v = n;
  while (v >= 0x80n) {
    out.push(Number(v & 0x7fn) | 0x80);
    v >>= 7n;
  }
  out.push(Number(v));
  return out;
};
const zigzag = (n: bigint) => varint(n >= 0n ? n << 1n : (-n << 1n) - 1n);
const header = (id: number) => [0, (id >>> 24) & 255, (id >>> 16) & 255, (id >>> 8) & 255, id & 255];
const utf8 = (s: string) => [...new TextEncoder().encode(s)];

/** Avro `{id, note: ["null","string"]}`, Protobuf `Payment {id, cents}` and JSON text, framed as a Confluent serializer frames them. */
const avro = (id: number, order: bigint, note: string) => Buffer.from([...header(id), ...zigzag(order), ...zigzag(1n), ...zigzag(BigInt(utf8(note).length)), ...utf8(note)]);
const proto = (id: number, pid: string, cents: bigint) => Buffer.from([...header(id), 0, 0x0a, utf8(pid).length, ...utf8(pid), 0x10, ...varint(cents)]);
const json = (id: number, text: string) => Buffer.from([...header(id), ...utf8(text)]);

type World = { topic: string; subject: string; avroId: number; jsonSubject: string; protoSubject: string };

/** A topic of the browser's own with an Avro, a JSON Schema and a Protobuf record on it, and their subjects. */
async function world(page: Page, browser: string): Promise<World> {
  const tag = `${browser}-${Date.now() % 100000}`;
  const topic = `e2e.schemas.${tag}`;
  const url = await registryUrl(page.request);
  const subject = `${topic}-value`;
  const avroId = await registerRest(page.request, url, subject, "AVRO", AVRO_NOTE);
  const jsonSubject = `e2e-stock-${tag}`;
  const protoSubject = `e2e-pay-${tag}`;
  const jsonId = await registerRest(page.request, url, jsonSubject, "JSON", JSON_SCHEMA);
  const protoId = await registerRest(page.request, url, protoSubject, "PROTOBUF", PROTO);
  const kafka = new Kafka({ clientId: "e2e-schemas", brokers: [await kafkaAddress(page.request)], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
  await admin.disconnect();
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  await p.send({
    topic,
    messages: [
      { key: "a", value: avro(avroId, 42n, "rush") },
      { key: "j", value: json(jsonId, '{"sku":"A-1","qty":2}') },
      { key: "p", value: proto(protoId, "p-9", 1250n) },
    ],
  });
  await p.disconnect();
  return { topic, subject, avroId, jsonSubject, protoSubject };
}

test("The sidebar counts subjects, and the list keeps its filters in the address", async ({ page, browserName }) => {
  const w = await world(page, browserName);
  await page.goto("/");
  const item = page.getByTestId("sidebar").getByRole("link", { name: /Schemas/ });
  await expect(page.getByTestId("count-schemas")).toHaveText(/^\d+$/);
  await item.click();
  await expect(page).toHaveURL(/\/schemas$/);
  await expect(page.getByRole("heading", { level: 1, name: "Schemas" })).toBeVisible();
  await page.getByTestId("schemas-filter").click();
  await page.keyboard.type(w.topic);
  await expect(page).toHaveURL(new RegExp(`q=${w.topic.replace(/\./g, "\\.")}`));
  await expect(page.getByTestId("subject-link")).toHaveText([w.subject]);
  // The type filter: Protobuf only leaves nothing of this topic's.
  await page.getByRole("combobox", { name: "Types" }).click();
  await page.getByRole("option", { name: /Protobuf/ }).click();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/type=PROTOBUF/);
  await expect(page.getByTestId("subject-link")).toHaveCount(0);
  await page.goto(`/schemas?q=${encodeURIComponent(w.protoSubject)}`);
  await expect(page.getByTestId("subject-link")).toHaveText([w.protoSubject]);
  await page.getByTestId("show-deleted").click();
  await expect(page).toHaveURL(/deleted=1/);
  // The subject's line names the topic its name points at.
  await page.goto(`/schemas?q=${encodeURIComponent(w.topic)}`);
  await expect(page.getByRole("row", { name: new RegExp(w.subject) })).toContainText(`value of ${w.topic}`);
  for (const width of [1024, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await overflowX(page), `no sideways scroll at ${width}`).toBe(0);
  }
});

test("Register: the live check explains a refusal in Confluent's words, the fix makes it pass, and the version lands", async ({ page, browserName }) => {
  const subject = `e2e-reg-${browserName}-${Date.now() % 100000}`;
  await page.goto("/schemas/new");
  await page.getByTestId("subject").click();
  await page.keyboard.type(subject);
  const schema = page.getByTestId("schema");
  await schema.fill(AVRO_V1);
  await expect(page.getByTestId("check-first")).toBeVisible();
  await page.getByTestId("register").click();
  await expect(page).toHaveURL(new RegExp(`/schemas/subjects/${subject}$`));
  await expect(page.getByTestId("version-shown")).toContainText("Version 1");

  await page.getByTestId("new-version").click();
  await expect(page).toHaveURL(/\/schemas\/new\?subject=/);
  await expect(page.getByTestId("subject")).toHaveValue(subject);
  await expect(page.getByTestId("schema")).toHaveValue(/"id"/);
  await page.getByTestId("schema").fill(AVRO_V2_BAD);
  const finding = page.getByTestId("finding").first();
  await expect(finding).toContainText("READER_FIELD_MISSING_DEFAULT_VALUE");
  await expect(finding).toContainText("The field 'qty' at path '/fields/1' in the new schema has no default value and is missing in the old schema");
  // Registering anyway is refused with 409 and nothing is stored.
  await page.getByTestId("register").click();
  await expect(page.getByTestId("schema-refused")).toContainText("Refused 409");
  await page.getByTestId("apply-fix").click();
  await expect(page.getByTestId("schema")).toHaveValue(/"default": 0/);
  await expect(page.getByTestId("check-compatible")).toBeVisible();
  await page.getByTestId("register").click();
  await expect(page).toHaveURL(new RegExp(`/schemas/subjects/${subject}$`));
  await expect(page.getByTestId("version-shown")).toContainText("Version 2");
  await expect(page.getByTestId("schema-text")).toContainText('{"name": "qty", "type": "int", "default": 0}');

  // The versions, the diff, and each level's verdict.
  await page.getByTestId("version-link").filter({ hasText: "v1" }).click();
  await expect(page).toHaveURL(/version=1/);
  await expect(page.getByTestId("version-shown")).toContainText("Version 1");
  await page.getByTestId("open-diff").click();
  await expect(page.getByTestId("diff-counts")).toHaveText("v1 to v2 · +3 −1");
  await page.getByTestId("diff-layout").getByRole("radio", { name: "Unified" }).click();
  await expect(page).toHaveURL(/layout=unified/);
  await expect(page.getByTestId("diff-unified").locator("tr[data-op=add]")).toHaveCount(3);
  await expect(page.getByTestId("level-check")).toHaveCount(3);
  await expect(page.getByTestId("level-check").filter({ hasText: "BACKWARD" })).toHaveAttribute("data-ok", "true");
});

test("A subject's compatibility, Try and deletes: overlays with their own address, focus back to the opener", async ({ page, browserName }) => {
  const w = await world(page, browserName);
  await page.goto(`/schemas/subjects/${encodeURIComponent(w.subject)}`);
  const change = page.getByTestId("open-compatibility");
  await change.focus();
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/dialog=compatibility/);
  const d = page.getByTestId("dialog-compatibility");
  await d.getByText("NONE", { exact: true }).click();
  await d.getByTestId("save-compatibility").click();
  await expect(d).toHaveCount(0);
  await expect(page.getByTestId("subject-level")).toHaveText("NONE");
  await expect(change).toBeFocused();

  const tryIt = page.getByTestId("open-try");
  await tryIt.click();
  const value = page.getByTestId("try-value");
  await expect(value).toHaveValue(/"id": 0/);
  await value.fill('{"id": "x"}');
  await expect(page.getByTestId("try-error")).toHaveText('id: expected a number, not "x"');
  await value.fill('{"id": 7, "note": "hi"}');
  await expect(page.getByTestId("try-hex")).toHaveText(new RegExp(`^00 00 00 00 ${w.avroId.toString(16).padStart(2, "0")} 0e 02 04 68 69$`));
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("try-sheet")).toHaveCount(0);
  await expect(tryIt).toBeFocused();

  // Soft delete, then for good with the name typed.
  await page.getByTestId("open-delete-subject").click();
  await page.getByTestId("delete-subject-submit").click();
  await expect(page.getByTestId("subject-pills")).toContainText("soft-deleted");
  await page.getByTestId("open-delete-subject").click();
  const submit = page.getByTestId("delete-subject-submit");
  await expect(submit).toBeDisabled();
  await page.getByTestId("confirm-subject").click();
  await page.keyboard.type(w.subject);
  await submit.click();
  await expect(page).toHaveURL(/\/schemas$/);
});

test("Records in the wire format are decoded with their schemas, in the table and the record Sheet", async ({ page, browserName }) => {
  const w = await world(page, browserName);
  await page.goto(`/topics/${encodeURIComponent(w.topic)}/messages`);
  const cells = page.getByTestId("schema-cell");
  await expect(cells).toHaveCount(3);
  await expect(cells.filter({ hasText: `AVRO · ${w.avroId}` })).toContainText('{"id":42,"note":{"string":"rush"}}');
  await expect(cells.filter({ hasText: "PROTOBUF" })).toContainText('{"id":"p-9","cents":"1250"}');
  await expect(cells.filter({ hasText: "JSON" })).toContainText('{"sku":"A-1","qty":2}');
  await page.getByRole("row", { name: /PROTOBUF/ }).getByTestId("open-record").click();
  const sheet = page.getByTestId("record-sheet");
  await expect(sheet.getByTestId("record-value")).toHaveText('{\n  "id": "p-9",\n  "cents": "1250"\n}');
  await expect(sheet).toContainText("as shop.Payment");
  await expect(sheet.getByTestId("record-value-subject")).toHaveText(`${w.protoSubject} v1`);
  await sheet.getByTestId("record-value-view").getByRole("radio", { name: "Bytes" }).click();
  await expect(sheet.getByTestId("record-value")).toContainText("00 00 00 00");
  await page.keyboard.press("Escape");
  // The topic's Schema tab: the subject named for it, and the two its records carry.
  await page.locator("[data-tab=schema]").click();
  await expect(page.getByTestId("topic-subject")).toHaveCount(3);
  await expect(page.getByTestId("topic-subject").first()).toContainText("its values");
});

test("Produce encodes a JSON value with the topic's subject, shows the bytes, and the record reads back", async ({ page, browserName }) => {
  const w = await world(page, browserName);
  await page.goto(`/topics/${encodeURIComponent(w.topic)}/messages`);
  await page.getByTestId("act-produce").click();
  const dialog = page.getByTestId("dialog-produce");
  await expect(dialog.getByTestId("schema")).toContainText(`encode with ${w.subject}`);
  await dialog.getByTestId("value").click();
  await page.keyboard.type('{"id": "nope"}');
  await expect(dialog.getByTestId("encoded-error")).toContainText('id: expected a number, not "nope"');
  await dialog.getByTestId("value").fill('{"id": 99}');
  await expect(dialog.getByTestId("encoded-hex")).toHaveText(/^00 00 00 00 [0-9a-f]{2} c6 01 00$/);
  await dialog.getByTestId("send").click();
  await expect(dialog.getByTestId("produced")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("schema-cell").filter({ hasText: '{"id":99,"note":null}' })).toHaveCount(1);
});

test("Connect names the registry and the topics list its subjects", async ({ page, browserName }) => {
  const w = await world(page, browserName);
  await page.goto("/connect");
  const reg = page.getByTestId("connect-registry");
  await expect(reg).toContainText("schema.registry.url=http://");
  await page.goto(`/topics?q=${encodeURIComponent(w.topic)}`);
  await expect(page.getByTestId("topic-subject")).toHaveText([w.subject]);
  await page.getByTestId("topic-subject").click();
  await expect(page).toHaveURL(new RegExp(`/schemas/subjects/${w.topic.replace(/\./g, "\\.")}-value$`));
  await apiPost(page.request, (await page.evaluate(() => location.origin)) as string, `/api/v1/schemas/subjects/${encodeURIComponent(w.subject)}/delete`, {});
});
