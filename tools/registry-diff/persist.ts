// doze-kafka's registry across a restart (TERM) and a kill (KILL): everything it answered before is answered the same
// after, the next id is not one given before, and a registration answered just before a kill is kept.
//
//   bun tools/registry-diff/persist.ts      (the stack up: stack.sh up; uses stack.sh doze-restart and doze-kill)
import { join } from "node:path";
import { j, reg } from "./lib.ts";

const doze = process.env.REGDIFF_DOZE ?? "http://127.0.0.1:40011";
const stack = join(import.meta.dir, "stack.sh");
const H = { "Content-Type": "application/vnd.schemaregistry.v1+json" };
let failed = 0;
const check = (name: string, ok: boolean, detail = "") => {
  if (!ok) failed++;
  console.log(`${ok ? "ok" : "FAIL"} ${name}${detail ? ` ${detail}` : ""}`);
};
const req = async (m: string, p: string, b?: string) => {
  const r = await fetch(doze + p, { method: m, headers: H, body: b });
  return { status: r.status, body: await r.text() };
};
const sh = (arg: string) => {
  const r = Bun.spawnSync(["bash", stack, arg], { stdout: "pipe", stderr: "pipe" });
  if (r.exitCode !== 0) throw new Error(`stack.sh ${arg}: ${r.stderr}`);
};

const p = `persist-${Date.now()}`;
const ids: number[] = [];
const add = async (s: string, body: string) => ids.push(JSON.parse((await req("POST", `/subjects/${p}-${s}/versions`, body)).body).id);
await add("a", reg({ type: "record", name: "A", fields: [{ name: "x", type: "int" }] }));
await add("a", reg({ type: "record", name: "A", fields: [{ name: "x", type: "int" }, { name: "y", type: "string", default: "" }] }));
await add("p", reg('syntax = "proto3";\nmessage P { string s = 1; }\n', { schemaType: "PROTOBUF" }));
await add("j", reg(j({ type: "object", properties: { n: { type: "number" } } }), { schemaType: "JSON" }));
await add("r", reg({ type: "record", name: "R", fields: [{ name: "a", type: "A" }] }, { references: [{ name: "A", subject: `${p}-a`, version: 1 }] }));
await add("gone", reg({ type: "fixed", name: "F", size: 2 }));
await req("DELETE", `/subjects/${p}-a/versions/2`);
await req("DELETE", `/subjects/${p}-gone`);
await req("DELETE", `/subjects/${p}-gone?permanent=true`);
await req("PUT", `/config/${p}-a`, j({ compatibility: "FULL", normalize: true, alias: "x" }));
await req("PUT", `/mode/${p}-p`, j({ mode: "READONLY" }));
const views = [
  `/subjects?deleted=true&subjectPrefix=${p}`,
  `/subjects/${p}-a/versions?deleted=true`,
  `/subjects/${p}-a/versions/2?deleted=true`,
  `/subjects/${p}-r/versions/1`,
  `/subjects/${p}-a/versions/1/referencedby`,
  `/config/${p}-a`,
  `/mode/${p}-p`,
  ...ids.map((id) => `/schemas/ids/${id}`),
  `/schemas?subjectPrefix=${p}&deleted=true`,
];
const snapshot = async () => Promise.all(views.map(async (v) => `${v} ${j(await req("GET", v))}`));
const maxBefore = Math.max(...ids);

for (const how of ["doze-restart", "doze-kill"] as const) {
  // Registered just before the stop: it must be there after.
  const last = JSON.parse((await req("POST", `/subjects/${p}-${how}/versions`, reg({ type: "fixed", name: `L${p.replace(/\D/g, "")}x${how.length}`, size: 1 }))).body).id;
  const before = await snapshot();
  sh(how);
  const after = await snapshot();
  const same = after.every((a, i) => a === before[i]);
  check(`${how}: every answer is the same`, same, same ? "" : j(after.filter((a, i) => a !== before[i])));
  const kept = await req("GET", `/subjects/${p}-${how}/versions/1`);
  check(`${how}: the last registration is kept`, kept.status === 200 && JSON.parse(kept.body).id === last);
  const next = JSON.parse((await req("POST", `/subjects/${p}-next-${how}/versions`, reg({ type: "enum", name: `E${p.replace(/\D/g, "")}x${how.length}`, symbols: ["A"] }))).body).id;
  check(`${how}: the next id is new`, next > maxBefore && next > last && !ids.includes(next), `${next}`);
  ids.push(next);
}
// The id spent by the permanent delete is not given again.
check("a permanently deleted id is not given again", (await req("GET", `/schemas/ids/${ids[5]}`)).status === 404);
console.log(failed === 0 ? "all ok" : `${failed} failed`);
process.exit(failed === 0 ? 0 : 1);
