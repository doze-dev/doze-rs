// The `consoles` fixture: one instance of every module that declares a console, each with the spec it publishes and
// rows of the size it sends, so the app's console blocks are seen against every real module at once (and against a
// module newer than the app). bun run apps/doze-app/fixtures/consoles.ts > apps/doze-app/fixtures/scripts/consoles.ndjson
//
// Where each spec is from. The six real modules' specs and actions are read from the snapshot each module's own test
// pins (`console.snapshot.json`, rewritten with DOZE_UPDATE_SNAPSHOT=1 when the module's console changes; the test
// fails until it is), so this fixture is never a hand copy of what a module publishes. Only the rows are made up here,
// with the cells each module's `resources` sends.
//   store    kvrocks   crates/doze-module-kvrocks/src/console.snapshot.json
//   cache    valkey    crates/doze-module-valkey/src/console.snapshot.json
//   flows    temporal  crates/doze-module-temporal/src/console.snapshot.json
//   db       postgres  crates/doze-module-postgres/src/console.snapshot.json
//   events   kafka     crates/doze-kafka/src/module/console.snapshot.json
//   files    aws       crates/doze-aws/src/module/console.snapshot.json (a web console only)
//   echo     echo      crates/doze-module/src/bin/echo.rs       console() (the Rust test module)
//   echo-go  echo      doze-sdk/plugin/echo/main.go             DescribeConsole() (the Go test module)
//   gauge    (none)    a module newer than this doze: a 1,000-row table, a busy stream, a chart, a note, and a block
//                      kind no doze knows yet

const root = new URL("../../../", import.meta.url).pathname;
const snapshot = async (path: string): Promise<{ spec: Spec; actions: Action[] }> => Bun.file(root + path).json();
const kvrocks = await snapshot("crates/doze-module-kvrocks/src/console.snapshot.json");
const valkey = await snapshot("crates/doze-module-valkey/src/console.snapshot.json");
const temporal = await snapshot("crates/doze-module-temporal/src/console.snapshot.json");
const postgres = await snapshot("crates/doze-module-postgres/src/console.snapshot.json");
const kafka = await snapshot("crates/doze-kafka/src/module/console.snapshot.json");
const aws = await snapshot("crates/doze-aws/src/module/console.snapshot.json");

type Cells = Record<string, string>;
type Col = { key: string; title: string; align?: string; role?: string };
type Panel = { id: string; title: string; kind: string; source?: string; columns: Col[]; select?: string };
type Node = { kind: string; id?: string; title?: string; weight?: number; panel?: string; from?: string; text?: string; actions?: string[]; children?: Node[] };
type Spec = { front?: string; web_url?: string; panels: Panel[]; tabs?: { id: string; title: string; body: Node }[]; extend?: { after: string; node: Node }[] };
type Action = { id: string; label: string; kind: string; destructive?: boolean; input_hint?: string };

const epoch = Date.UTC(2026, 9, 11, 14, 4, 40);
const at = (agoSec: number) => new Date(epoch - agoSec * 1000).toISOString().replace(".000Z", "Z");
const zero = "0001-01-01T00:00:00Z";
const c = (key: string, title: string, role = "", align = ""): Col => {
  const col: Col = { key, title };
  // Figures on the right, words (and times, states, code) on the left, as the Rust modules align them.
  const right = ["count", "rate", "bytes", "lag", "of"].includes(role);
  if (align || role) col.align = align || (right ? "right" : "left");
  if (role) col.role = role;
  return col;
};

const instances: any[] = [];
const frames: any[] = [];
const out: string[] = [];

function module(name: string, engine: string, version: string, pid: number, mb: number, conns: number, front = "", state = "active") {
  instances.push({
    name, kind: "module", engine, version, state, pid: state === "active" ? pid : 0, conns, ram: state === "active" ? mb << 20 : 0, cpu: state === "active" ? 0.4 : 0,
    started_at: at(600), idle_since: zero, next_restart: zero, declared: true,
    domain: `${name}.consoles.doze`, endpoint: `127.0.0.1:${7000 + pid % 1000}`, bind: `127.0.0.1:${7000 + pid % 1000}`,
    holders: [{ kind: "connection", count: conns }], has_console: true, console_front: front,
  });
}

function console(inst: string, spec: Spec, actions: Action[], tables: Record<string, Cells[]>, streams: Record<string, Cells[]> = {}, state = "active") {
  frames.push({ instance: inst, state, spec, actions });
  for (const [panel, rows] of Object.entries(tables)) frames.push({ instance: inst, panel, rows: rows.map((cells) => ({ cells })) });
  for (const [panel, rows] of Object.entries(streams)) for (const cells of rows) frames.push({ instance: inst, panel, row: { cells } });
}

const clock = (agoSec: number) => new Date(epoch - agoSec * 1000).toISOString().slice(11, 19);

// kvrocks
module("store", "kvrocks", "2.11", 760, 40, 2);
console("store", kvrocks.spec, kvrocks.actions, {
  server: [{ memory: "38.2 MB", disk: "412 MB", keys: "18204", clients: "2", hits: "90311" }],
  namespaces: [{ name: "(default)", keys: "12011" }, { name: "sessions", keys: "5880" }, { name: "rate-limits", keys: "313" }],
}, {
  commands: Array.from({ length: 360 }, (_, i) => ({ time: clock(360 - i), client: i % 3 ? "sessions · 127.0.0.1:52144" : "(default) · 127.0.0.1:52101", command: i % 4 ? `GET session:${4000 + i}` : `SETEX session:${4000 + i} 3600 {"user":${i}}` })),
});

// valkey
module("cache", "valkey", "9", 701, 11, 1);
console("cache", valkey.spec, valkey.actions, {
  server: [{ memory: "11.0 MB", ceiling: "256 MB", keys: "2041", clients: "1", hits: "5521", evicted: "0", persistence: "snapshot when it sleeps" }],
  databases: [{ name: "db0", keys: "2011", expiring: "1980" }, { name: "db1", keys: "30", expiring: "0" }],
}, {
  commands: Array.from({ length: 40 }, (_, i) => ({ time: clock(40 - i), client: "127.0.0.1:50311", command: i % 2 ? `GET cart:${i}` : `EXPIRE cart:${i} 1800` })),
});

// temporal
module("flows", "temporal", "1.1", 733, 148, 1, "web");
console("flows", temporal.spec, temporal.actions, {
  namespaces: [{ name: "default", status: "registered", retention: "7 days", running: "3" }, { name: "billing", status: "registered", retention: "36 hours", running: "0" }, { name: "old-imports", status: "deprecated", retention: "90 minutes", running: "0" }],
});
instances[instances.length - 1].web_url = "http://127.0.0.1:8233";

// postgres
module("db", "postgres", "17", 612, 96, 4, "overview");
console("db", postgres.spec, postgres.actions, {
  activity: [{ active: "3", idle: "12", waiting: "1", connections: "16", of: "100", longest: "4.2 s" }],
  // The sessions the figures above count: 1 waiting (the longest, 4.2 s), 3 active, 12 idle.
  sessions: Array.from({ length: 16 }, (_, i) => ({
    pid: String(4412 - i), user: "app", database: i % 5 ? "app" : "app_test", app: ["api", "worker", "psql", "migrate"][i % 4],
    for: i === 0 ? "4.2 s" : `${(((i * 7) % 40) / 10).toFixed(1)} s`, state: i === 0 ? "waiting" : i < 4 ? "active" : "idle",
    query: i % 3 ? "SELECT o.id, o.total, c.email FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.status = $1 AND o.created_at > now() - interval '7 days' ORDER BY o.created_at DESC LIMIT 50" : "UPDATE orders SET status = $1 WHERE id = $2",
  })),
  databases: [{ database: "app", size: "40121344", connections: "4", owner: "postgres" }, { database: "app_test", size: "9437184", connections: "0", owner: "postgres" }, { database: "postgres", size: "7602176", connections: "0", owner: "postgres" }],
});

// echo (Rust)
module("echo", "echo", "0.1", 801, 4, 0, "console");
console("echo", {
  front: "console",
  panels: [{ id: "channels", title: "Channels", kind: "counters", source: "channel", columns: [c("size", "Size", "count")] }],
  extend: [{ after: "vitals", node: { kind: "stat", panel: "channels", title: "Channels" } }],
}, [], { channels: [{ size: "3" }] });

// echo (Go)
module("echo-go", "echo", "0.1", 802, 6, 1, "console");
console("echo-go", {
  front: "console", web_url: "http://127.0.0.1:7802/_console",
  panels: [
    { id: "echoes", title: "Echoes", kind: "table", source: "echo", columns: [c("name", "Name"), c("sent", "Sent", "count")] },
    { id: "wire", title: "Wire", kind: "stream", select: "echoes", columns: [c("text", "Text")] },
  ],
}, [], { echoes: [{ name: "hello", sent: "12" }, { name: "ping", sent: "3" }] }, {
  wire: [{ _for: "hello", text: "hello" }, { _for: "hello", text: "hello again" }, { _for: "hello", text: "hello, world" }],
});

// kafka
module("events", "kafka", "4", 741, 188, 2, "console");
console("events", kafka.spec, kafka.actions, {
  topics: [{ name: "orders", partitions: "3", high: "1043" }, { name: "emails", partitions: "1", high: "87" }, { name: "orders.dlq", partitions: "1", high: "0", _runtime: "1" }],
  groups: [{ name: "worker", state: "Stable", members: "1", lag: "0" }, { name: "mailer", state: "Empty", members: "0", lag: "12" }, { name: "billing-reconciliation-consumers", state: "Stable", members: "3", lag: "0" }],
}, {
  tail: Array.from({ length: 320 }, (_, i) => ({ _for: "orders", time: clock(320 - i), partition: String(i % 3), offset: String(700 + i), key: `ord_${700 + i}`, value: `{"total": ${1000 + i * 7}, "items": ${1 + (i % 4)}}` })),
});
instances[instances.length - 1].web_url = "http://127.0.0.1:7741";

// aws: its console is its own web page (no panels)
module("files", "aws", "0.1", 752, 64, 1, "web");
console("files", aws.spec, aws.actions, {});
instances[instances.length - 1].web_url = "http://127.0.0.1:7752";

// The console's other states: asleep (what it last read is kept; the console never wakes it), still reading (no
// spec yet), and failed (the module did not answer).
module("archive", "valkey", "9", 704, 9, 0, "", "reaped");
console("archive", valkey.spec, valkey.actions, {
  server: [{ memory: "9.2 MB", ceiling: "no limit", keys: "812", clients: "0", hits: "120", evicted: "0", persistence: "snapshot when it sleeps" }],
  databases: [{ name: "db0", keys: "812", expiring: "0" }],
}, {}, "reaped");
module("pending", "echo", "0.1", 805, 3, 0, "console");
module("broken", "echo", "0.1", 806, 3, 0, "console");
frames.push({ instance: "broken", error: "the module did not answer: its console call timed out after 5s" });

// a module newer than this doze
module("gauge", "gauge", "0.9", 900, 22, 1, "console");
console("gauge", {
  front: "console",
  panels: [
    { id: "series", title: "Requests a second", kind: "table", source: "point", columns: [c("t", "Time"), c("rps", "Requests", "count")] },
    { id: "keys", title: "Keys", kind: "table", source: "key", columns: [c("key", "Key"), c("type", "Type"), c("size", "Size", "bytes"), c("ttl", "TTL", "", "right"), c("touched", "Touched", "", "right")] },
    { id: "log", title: "Requests", kind: "stream", columns: [c("time", "Time", "time"), c("method", "Method"), c("path", "Path"), c("status", "Status", "state", "right"), c("took", "Took", "", "right")] },
    { id: "errors", title: "Errors", kind: "stream", columns: [c("time", "Time", "time"), c("path", "Path"), c("error", "Error")] },
    // A bytes figure with a bytes limit, and a figure whose limit the module sent empty.
    { id: "mem", title: "Memory", kind: "counters", source: "server", columns: [c("used", "Used", "bytes"), c("max", "Max", "of"), c("clients", "Clients", "count"), c("cap", "Cap", "of")] },
  ],
  tabs: [{ id: "overview", title: "overview", body: { kind: "col", children: [
    { kind: "stat", panel: "mem", id: "mem", title: "Memory" },
    { kind: "row", children: [
      { kind: "chart", panel: "series", id: "chart-rps", title: "Requests a second", weight: 2 },
      { kind: "note", id: "note", title: "About", text: "gauge keeps every key in memory and writes a snapshot when it sleeps. Keys with a TTL leave on their own.", weight: 1 },
      { kind: "gauge", id: "dial", title: "Memory", panel: "series", weight: 1 },
    ] },
    { kind: "table", panel: "keys", id: "keys" },
  ] } },
  { id: "requests", title: "requests", body: { kind: "col", children: [{ kind: "stream", panel: "log", id: "log", weight: 1 }] } },
  { id: "errors", title: "errors", body: { kind: "col", children: [{ kind: "stream", panel: "errors", id: "errors", weight: 1 }, { kind: "note", id: "why", title: "About", text: "A request that fails is written here as it happens; none has yet." }] } }],
  extend: [{ after: "vitals", node: { kind: "row", children: [
    { kind: "chart", panel: "series", title: "Requests a second", weight: 1 },
    { kind: "sparkle", title: "Something new", weight: 1 },
  ] } }],
}, [{ id: "evict", label: "Evict this key", kind: "key", destructive: true }], {
  mem: [{ used: "67108864", max: "268435456", clients: "7", cap: "" }],
  series: Array.from({ length: 120 }, (_, i) => ({ t: clock(120 - i), rps: String(Math.round(140 + 60 * Math.sin(i / 9) + (i % 17 === 0 ? 120 : 0))) })),
  keys: Array.from({ length: 1000 }, (_, i) => ({
    key: `user:${String(10000 + i)}:profile${i % 7 === 0 ? ":with-a-much-longer-name-than-the-rest-of-them" : ""}`,
    type: ["hash", "string", "list", "set"][i % 4], size: String((i * 977) % 90000 + 120), ttl: i % 3 ? "—" : `${(i % 59) + 1}m`, touched: `${(i % 50) + 1}s ago`,
  })),
}, {
  log: Array.from({ length: 400 }, (_, i) => ({ time: clock(400 - i), method: i % 5 ? "GET" : "POST", path: i % 5 ? `/users/${10000 + (i % 1000)}` : "/sessions", status: i % 23 === 0 ? "500" : "200", took: `${(i * 13) % 90 + 1}ms` })),
});

const status = { ok: true, idle_timeout: 300000000000, stack: "consoles", dir: "~/code/consoles", instances };
out.push(JSON.stringify({ status }));
out.push(JSON.stringify({ hold: true }));
const stats = instances.map((v) => ({ name: v.name, pid: v.pid, ram: v.ram, cpu: v.cpu, conns: v.conns }));
for (let i = 0; i < 5; i++) out.push(JSON.stringify({ frame: { topic: "stats", at: zero, stats } }));
for (const f of frames) out.push(JSON.stringify({ frame: { topic: "console", at: zero, console: f } }));
await Bun.write(Bun.stdout, out.join("\n") + "\n");
