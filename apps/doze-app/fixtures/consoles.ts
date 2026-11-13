// The `consoles` fixture: one instance of every module that declares a console, each with the spec it publishes and
// rows of the size it sends, so the app's console blocks are seen against every real module at once (and against a
// module newer than the app). bun run apps/doze-app/fixtures/consoles.ts > apps/doze-app/fixtures/scripts/consoles.ndjson
//
// Where each spec is from:
//   store    kvrocks   crates/doze-module-kvrocks/src/admin.rs  console(), actions()
//   cache    valkey    crates/doze-module-valkey/src/admin.rs   console(), actions()
//   flows    temporal  crates/doze-module-temporal/src/admin.rs console()
//   db       postgres  doze-modules/modules/postgres/console.go DescribeConsole() (Go; not ported yet)
//   echo     echo      crates/doze-module/src/bin/echo.rs       console() (the Rust test module)
//   echo-go  echo      doze-sdk/plugin/echo/main.go             DescribeConsole() (the Go test module)
//   events   kafka     doze internal/control/fixture KafkaConsole() (what Go's dash was drawn against)
//   files    aws       doze internal/control/fixture AwsConsole()
//   gauge    (none)    a module newer than this doze: a 1,000-row table, a busy stream, a chart, a note, and a block
//                      kind no doze knows yet

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
  if (align || role) col.align = align || "right";
  if (role) col.role = role;
  return col;
};

const instances: any[] = [];
const frames: any[] = [];
const out: string[] = [];

function module(name: string, engine: string, version: string, pid: number, mb: number, conns: number, front = "") {
  instances.push({
    name, kind: "module", engine, version, state: "active", pid, conns, ram: mb << 20, cpu: 0.4,
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
console("store", {
  panels: [
    { id: "server", title: "Kvrocks", kind: "counters", source: "server", columns: [c("memory", "Memory"), c("disk", "Disk"), c("keys", "Keys", "count"), c("clients", "Clients", "count"), c("hits", "Hits", "rate")] },
    { id: "namespaces", title: "Namespaces", kind: "table", source: "namespace", columns: [c("name", "Namespace"), c("keys", "Keys", "count")] },
    { id: "commands", title: "Commands", kind: "stream", columns: [c("time", "Time", "time"), c("client", "Namespace and client"), c("command", "Command", "code")] },
  ],
  extend: [{ after: "vitals", node: { kind: "row", children: [
    { kind: "stat", panel: "server", title: "Kvrocks", weight: 1 },
    { kind: "table", panel: "namespaces", title: "Namespaces", weight: 1 },
    { kind: "actions", title: "Actions", weight: 1 },
  ] } }],
}, [
  { id: "flush", label: "Empty this namespace", kind: "namespace", destructive: true },
  { id: "recount", label: "Count keys again", kind: "server" },
  { id: "compact", label: "Compact (free deleted data's disk)", kind: "server" },
], {
  server: [{ memory: "38.2 MB", disk: "412 MB", keys: "18204", clients: "2", hits: "90311" }],
  namespaces: [{ name: "(default)", keys: "12011" }, { name: "sessions", keys: "5880" }, { name: "rate-limits", keys: "313" }],
}, {
  commands: Array.from({ length: 360 }, (_, i) => ({ time: clock(360 - i), client: i % 3 ? "sessions · 127.0.0.1:52144" : "(default) · 127.0.0.1:52101", command: i % 4 ? `GET session:${4000 + i}` : `SETEX session:${4000 + i} 3600 {"user":${i}}` })),
});

// valkey
module("cache", "valkey", "9", 701, 11, 1);
console("cache", {
  panels: [
    { id: "server", title: "Valkey", kind: "counters", source: "server", columns: [c("memory", "Memory"), c("ceiling", "Ceiling"), c("keys", "Keys", "count"), c("clients", "Clients", "count"), c("hits", "Hits", "rate"), c("evicted", "Evictions", "rate"), c("persistence", "Keeps data")] },
    { id: "databases", title: "Databases", kind: "table", source: "database", columns: [c("name", "Database"), c("keys", "Keys", "count"), c("expiring", "Expiring", "count")] },
    { id: "commands", title: "Commands", kind: "stream", columns: [c("time", "Time", "time"), c("client", "Client"), c("command", "Command", "code")] },
  ],
  extend: [{ after: "vitals", node: { kind: "row", children: [{ kind: "stat", panel: "server", title: "Valkey", weight: 1 }, { kind: "actions", title: "Actions", weight: 1 }] } }],
}, [
  { id: "flush", label: "Empty the cache", kind: "server", destructive: true },
  { id: "save", label: "Save a snapshot now", kind: "server" },
], {
  server: [{ memory: "11.0 MB", ceiling: "no limit", keys: "2041", clients: "1", hits: "5521", evicted: "0", persistence: "snapshot when it sleeps" }],
  databases: [{ name: "db0", keys: "2011", expiring: "1980" }, { name: "db1", keys: "30", expiring: "0" }],
}, {
  commands: Array.from({ length: 40 }, (_, i) => ({ time: clock(40 - i), client: "127.0.0.1:50311", command: i % 2 ? `GET cart:${i}` : `EXPIRE cart:${i} 1800` })),
});

// temporal
module("flows", "temporal", "1.1", 733, 148, 1, "web");
console("flows", {
  front: "web", web_url: "http://127.0.0.1:8233",
  panels: [{ id: "namespaces", title: "Namespaces", kind: "table", source: "namespace", columns: [c("name", "Namespace"), c("status", "State", "state", "left"), c("retention", "Retention"), c("running", "Running", "count")] }],
  extend: [{ after: "vitals", node: { kind: "table", panel: "namespaces", title: "Namespaces", weight: 1 } }],
}, [], {
  namespaces: [{ name: "default", status: "registered", retention: "7 days", running: "3" }, { name: "billing", status: "registered", retention: "36 hours", running: "0" }, { name: "old-imports", status: "deprecated", retention: "90 minutes", running: "0" }],
});
instances[instances.length - 1].web_url = "http://127.0.0.1:8233";

// postgres (Go)
module("db", "postgres", "17", 612, 96, 4, "overview");
console("db", {
  front: "overview",
  panels: [
    { id: "activity", title: "", kind: "counters", source: "activity", columns: [c("active", "active"), c("idle", "idle"), c("waiting", "waiting"), c("of", "of"), c("longest", "longest")] },
    { id: "sessions", title: "sessions", kind: "table", source: "session", columns: [c("pid", "pid", "", "right"), c("user", "user"), c("database", "database"), c("app", "app"), c("for", "for", "", "right"), c("state", "state", "state", "left"), c("query", "query", "detail", "left")] },
    { id: "databases", title: "databases", kind: "table", source: "database", columns: [c("database", "database"), c("size", "size", "bytes"), c("connections", "connections", "count"), c("owner", "owner")] },
  ],
  tabs: [{ id: "activity", title: "activity", body: { kind: "col", children: [
    { kind: "stat", panel: "activity", id: "stat-activity" },
    { kind: "row", weight: 1, children: [
      { kind: "table", panel: "sessions", id: "sessions", weight: 2 },
      { kind: "col", weight: 1, children: [{ kind: "detail", panel: "sessions", from: "sessions", title: "query", id: "detail-sessions" }, { kind: "actions", id: "actions" }] },
    ] },
  ] } }],
  extend: [{ after: "vitals", node: { kind: "table", panel: "databases", id: "table-databases" } }],
}, [
  { id: "cancel", label: "Cancel query", kind: "session" },
  { id: "terminate", label: "Terminate session", kind: "session", destructive: true },
], {
  activity: [{ active: "3", idle: "196", waiting: "1", of: "200", longest: "4.2 s" }],
  // LIMIT 200: the most a real one sends.
  sessions: Array.from({ length: 200 }, (_, i) => ({
    pid: String(4412 - i), user: "app", database: i % 5 ? "app" : "app_test", app: ["api", "worker", "psql", "migrate"][i % 4],
    for: `${(i * 0.7).toFixed(1)} s`, state: i === 0 ? "waiting" : i < 3 ? "active" : "idle",
    query: i % 3 ? "SELECT o.id, o.total, c.email FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.status = $1 AND o.created_at > now() - interval '7 days' ORDER BY o.created_at DESC LIMIT 50" : "UPDATE orders SET status = $1 WHERE id = $2",
  })),
  databases: [{ database: "app", size: "40121344", connections: "4", owner: "postgres" }, { database: "app_test", size: "9437184", connections: "0", owner: "postgres" }],
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

// kafka (Go's dash fixture)
module("events", "kafka", "4", 741, 188, 2, "console");
console("events", {
  front: "console",
  panels: [
    { id: "topics", title: "topics", kind: "table", source: "topic", columns: [c("name", "name"), c("partitions", "parts", "count"), c("high", "messages", "count"), c("high", "rate", "rate")] },
    { id: "groups", title: "consumer groups", kind: "table", source: "group", columns: [c("name", "group"), c("topic", "topic"), c("lag", "lag", "lag"), c("members", "members")] },
    { id: "tail", title: "tail", kind: "stream", select: "topics", columns: [c("time", "time", "time"), c("partition", "p"), c("offset", "offset", "", "right"), c("key", "key"), c("value", "value")] },
  ],
}, [
  { id: "produce", label: "Produce a message", kind: "topic", input_hint: "message value" },
  { id: "keep", label: "Keep topic in doze.hcl", kind: "topic" },
], {
  topics: [{ name: "orders", partitions: "3", high: "1043" }, { name: "emails", partitions: "1", high: "87" }, { name: "orders.dlq", partitions: "1", high: "0", _runtime: "1" }],
  groups: [{ name: "worker", topic: "orders", lag: "0", members: "1" }, { name: "mailer", topic: "emails", lag: "12", members: "0" }],
}, {
  tail: Array.from({ length: 320 }, (_, i) => ({ _for: "orders", time: clock(320 - i), partition: String(i % 3), offset: String(700 + i), key: `ord_${700 + i}`, value: `{"total": ${1000 + i * 7}, "items": ${1 + (i % 4)}}` })),
});

// aws (Go's dash fixture)
module("files", "aws", "", 752, 64, 1, "console");
console("files", {
  front: "console",
  panels: [
    { id: "resources", title: "resources", kind: "table", source: "resource", columns: [c("type", "type"), c("name", "name"), c("now", "now"), c("from", "from")] },
    { id: "wire", title: "wire · every API call, newest first", kind: "stream", columns: [c("time", "time", "time"), c("svc", "svc"), c("action", "action"), c("code", "code", "state", "right"), c("target", "target")] },
  ],
}, [{ id: "keep", label: "Keep in doze.hcl", kind: "resource" }], {
  resources: [
    { type: "S3", name: "uploads", now: "12 objects · 3.1 MB", from: "doze.hcl" },
    { type: "SQS", name: "orders", now: "0 waiting · 2 in flight", from: "doze.hcl" },
    { type: "SQS", name: "orders-dlq", now: "1 waiting", from: "runtime", _runtime: "1" },
  ],
}, {
  wire: Array.from({ length: 60 }, (_, i) => ({ time: clock(60 - i), svc: i % 2 ? "SQS" : "S3", action: i % 2 ? "ReceiveMessage" : "PutObject", code: i % 9 === 0 ? "failed" : "ok", target: i % 2 ? "orders" : `upload/receipts/${1000 + i}.pdf` })),
});

// a module newer than this doze
module("gauge", "gauge", "0.9", 900, 22, 1, "console");
console("gauge", {
  front: "console",
  panels: [
    { id: "series", title: "Requests a second", kind: "table", source: "point", columns: [c("t", "Time"), c("rps", "Requests", "count")] },
    { id: "keys", title: "Keys", kind: "table", source: "key", columns: [c("key", "Key"), c("type", "Type"), c("size", "Size", "bytes"), c("ttl", "TTL", "", "right"), c("touched", "Touched", "", "right")] },
    { id: "log", title: "Requests", kind: "stream", columns: [c("time", "Time", "time"), c("method", "Method"), c("path", "Path"), c("status", "Status", "state", "right"), c("took", "Took", "", "right")] },
  ],
  tabs: [{ id: "overview", title: "overview", body: { kind: "col", children: [
    { kind: "row", children: [
      { kind: "chart", panel: "series", id: "chart-rps", title: "Requests a second", weight: 2 },
      { kind: "note", id: "note", title: "About", text: "gauge keeps every key in memory and writes a snapshot when it sleeps. Keys with a TTL leave on their own.", weight: 1 },
      { kind: "gauge", id: "dial", title: "Memory", panel: "series", weight: 1 },
    ] },
    { kind: "table", panel: "keys", id: "keys" },
  ] } },
  { id: "requests", title: "requests", body: { kind: "col", children: [{ kind: "stream", panel: "log", id: "log", weight: 1 }] } }],
  extend: [{ after: "vitals", node: { kind: "row", children: [
    { kind: "chart", panel: "series", title: "Requests a second", weight: 1 },
    { kind: "sparkle", title: "Something new", weight: 1 },
  ] } }],
}, [{ id: "evict", label: "Evict this key", kind: "key", destructive: true }], {
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
