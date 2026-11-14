// Seeds a broker and its registry for shots-schemas.ts: a topic `shop.orders` with Avro, JSON Schema and Protobuf records in the
// wire format, `shop.orders-value` with two versions, a key subject, a soft-deleted subject and one with a reference.
//   bun --bun seed-schemas.ts <kafka host:port> <registry url>
import { Kafka, logLevel } from "kafkajs";

const [kafkaAddr = "127.0.0.1:29310", registry = "http://127.0.0.1:29312"] = process.argv.slice(2);

async function reg(subject: string, schemaType: string, schema: string, references: unknown[] = []): Promise<number> {
  const r = await fetch(`${registry}/subjects/${encodeURIComponent(subject)}/versions`, { method: "POST", headers: { "Content-Type": "application/vnd.schemaregistry.v1+json" }, body: JSON.stringify({ schemaType, schema, references }) });
  if (!r.ok) throw new Error(`${subject}: ${r.status} ${await r.text()}`);
  return ((await r.json()) as { id: number }).id;
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
const zz = (n: bigint) => varint(n >= 0n ? n << 1n : (-n << 1n) - 1n);
const header = (id: number) => [0, (id >>> 24) & 255, (id >>> 16) & 255, (id >>> 8) & 255, id & 255];
const utf8 = (s: string) => [...new TextEncoder().encode(s)];
const str = (s: string) => [...zz(BigInt(utf8(s).length)), ...utf8(s)];

const V1 = '{"type":"record","name":"Order","namespace":"shop","fields":[{"name":"id","type":"long"},{"name":"customer","type":"string"},{"name":"total","type":"double"}]}';
const V2 = '{"type":"record","name":"Order","namespace":"shop","doc":"An order as the shop writes it","fields":[{"name":"id","type":"long"},{"name":"customer","type":"string"},{"name":"total","type":"double"},{"name":"note","type":["null","string"],"default":null},{"name":"state","type":{"type":"enum","name":"State","symbols":["NEW","PAID","SHIPPED"]},"default":"NEW"}]}';

await reg("shop.orders-value", "AVRO", V1);
const avroId = await reg("shop.orders-value", "AVRO", V2);
await reg("shop.orders-key", "AVRO", '"string"');
const jsonId = await reg("shop.stock-value", "JSON", '{"type":"object","properties":{"sku":{"type":"string"},"qty":{"type":"integer","minimum":0}},"required":["sku"],"additionalProperties":false}');
const protoId = await reg("shop.payments-value", "PROTOBUF", 'syntax = "proto3";\npackage shop.v1;\n\nmessage Payment {\n  string id = 1;\n  int64 cents = 2;\n  string currency = 3;\n}\n');
await reg("shop.Address", "AVRO", '{"type":"record","name":"Address","namespace":"shop","fields":[{"name":"city","type":"string"}]}');
await reg("shop.customers-value", "AVRO", '{"type":"record","name":"Customer","namespace":"shop","fields":[{"name":"name","type":"string"},{"name":"home","type":"shop.Address"}]}', [{ name: "shop.Address", subject: "shop.Address", version: 1 }]);
await reg("old.thing-value", "AVRO", '{"type":"record","name":"Old","fields":[{"name":"x","type":"int"}]}');
await fetch(`${registry}/subjects/old.thing-value`, { method: "DELETE" });

const kafka = new Kafka({ clientId: "seed-schemas", brokers: [kafkaAddr], logLevel: logLevel.NOTHING });
const admin = kafka.admin();
await admin.connect();
await admin.createTopics({ topics: [{ topic: "shop.orders", numPartitions: 2 }, { topic: "shop.payments", numPartitions: 1 }], waitForLeaders: true });
await admin.disconnect();
const p = kafka.producer({ allowAutoTopicCreation: false });
await p.connect();
const avro = (id: bigint, customer: string, total: number, note: string | null, state: number) => {
  const t = Buffer.alloc(8);
  t.writeDoubleLE(total);
  return Buffer.from([...header(avroId), ...zz(id), ...str(customer), ...t, ...(note === null ? [0] : [2, ...str(note)]), ...zz(BigInt(state))]);
};
await p.send({
  topic: "shop.orders",
  messages: [
    { key: "ord-1", value: avro(1n, "c-7710", 42.5, null, 0) },
    { key: "ord-2", value: avro(2n, "c-1203", 18, "leave at the door", 1) },
    { key: "ord-3", value: avro(3n, "c-7710", 99.99, null, 2) },
    { key: "stock", value: Buffer.from([...header(jsonId), ...utf8('{"sku":"A-1","qty":-2}')]) },
    { key: "raw", value: Buffer.from([...header(999), 1, 2, 3]) },
  ],
});
await p.send({ topic: "shop.payments", messages: [{ key: "pay-1", value: Buffer.from([...header(protoId), 0, 0x0a, 5, ...utf8("pay-1"), 0x10, ...varint(1250n), 0x1a, 3, ...utf8("EUR")]) }] });
await p.disconnect();
console.log("seeded");
