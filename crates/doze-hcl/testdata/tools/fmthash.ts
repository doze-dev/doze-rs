// go-fmtcases.jsonl: hclwrite.Format of every syntax case, as an FNV-1a 64 hash of the output bytes.
const fnv = (b: Uint8Array) => { let h = 0xcbf29ce484222325n; for (const x of b) h = ((h ^ BigInt(x)) * 0x100000001b3n) & 0xffffffffffffffffn; return h.toString(16).padStart(16, "0"); };
const lines = (await Bun.file(process.argv[2]).text()).trim().split("\n").map((l) => { const j = JSON.parse(l); return JSON.stringify({ id: j.id, h: fnv(new TextEncoder().encode(j.out)) }); });
await Bun.write(process.argv[3], lines.join("\n") + "\n");
