// Rewrites go-lex.jsonl and go-check.jsonl compactly (the Rust tests read this shape).
// Tokens are kept as a hash (FNV-1a, 64 bits) of their compact text, to keep the corpus small; on a
// difference, run `oracle-dump lex` on the case to see Go's tokens.
const fnv = (s: string) => {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(s)) h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
  return h.toString(16).padStart(16, "0");
};
const dir = process.argv[2];
const lex = (await Bun.file(`${dir}/go-lex.jsonl`).text()).trim().split("\n").map((l) => {
  const j = JSON.parse(l);
  const t = j.tokens.map((k: any[]) => `${String(k[0]).replace(/^Token/, "")} ${k[1]} ${k[2]} ${k[3]}:${k[4]}-${k[5]}:${k[6]}`).join("|");
  return JSON.stringify({ id: j.id, h: fnv(t) });
});
await Bun.write(`${dir}/go-lex.jsonl`, lex.join("\n") + "\n");
const chk = (await Bun.file(`${dir}/go-check.jsonl`).text()).trim().split("\n").map((l) => {
  const j = JSON.parse(l);
  const d = (j.r.diags ?? []).map((d: any) => `${d.at.start.line}:${d.at.start.col}-${d.at.end.line}:${d.at.end.col} ${d.summary}`);
  return JSON.stringify({ id: j.id, ok: j.r.ok, d });
});
await Bun.write(`${dir}/go-check.jsonl`, chk.join("\n") + "\n");
