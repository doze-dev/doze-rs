// compare-heartbeats.ts <kafka log> <doze log> [v]: the StreamsGroupHeartbeat answers each Kafka Streams client got (from
// interop/java/Streams.java run with JAVA_LOG=debug against each broker), normalised (member ids, process ids, the run suffix)
// and compared client by client; with `v`, the answers that differ.
const read = async (p: string) => {
  const text = await Bun.file(p).text();
  const out = new Map<string, string[]>();
  const ids = new Map<string, string>();
  const procs = new Map<string, string>();
  for (const line of text.split("\n")) {
    const m = line.match(/clientId=([^,]*), groupId=([^\]]*)\].*Received STREAMS_GROUP_HEARTBEAT response.*?StreamsGroupHeartbeatResponseData\((.*)$/);
    if (!m) continue;
    const group = m[2].replace(/-[a-z0-9]{3,5}$/, "");
    const run = m[2].match(/-([a-z0-9]{3,5})$/)?.[1] ?? "";
    const uuid = m[1].match(/[0-9a-f]{8}-[0-9a-f-]{27}/)?.[0] ?? "";
    if (!procs.has(m[2] + uuid)) procs.set(m[2] + uuid, "P" + [...procs.keys()].filter((x) => x.startsWith(m[2])).length);
    const client = m[1].replace(m[2], "APP").replace(uuid, procs.get(m[2] + uuid)!);
    let body = m[3].replaceAll(run, "RUN").replace(/memberId='[^']*'/, "").replace(/throttleTimeMs=0, /, "");
    body = body.replace(/heartbeatIntervalMs=5000, acceptableRecoveryLagLegacy=0, taskOffsetIntervalMs=60000, acceptableRecoveryLag=10000, /, "");
    const key = group + " " + client;
    if (!ids.has(key)) ids.set(key, "c" + ids.size);
    const list = out.get(key) ?? [];
    if (list[list.length - 1] !== body) list.push(body);
    out.set(key, list);
  }
  return out;
};
const [k, d] = [await read(process.argv[2]), await read(process.argv[3])];
for (const key of new Set([...k.keys(), ...d.keys()])) {
  const a = k.get(key) ?? [], b = d.get(key) ?? [];
  const same = a.length === b.length && a.every((x, i) => x === b[i]);
  console.log(`${same ? "same" : "DIFF"} ${key} (${a.length} vs ${b.length})`);
  if (!same && process.argv[4]) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      if (a[i] !== b[i]) { console.log("  K " + (a[i] ?? "-").slice(0, 600)); console.log("  D " + (b[i] ?? "-").slice(0, 600)); }
    }
  }
}
