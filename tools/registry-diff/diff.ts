// Sends the same requests to Confluent's registry and to doze-kafka's and compares the answers byte for byte (status,
// Content-Type, WWW-Authenticate, Allow, body; ids made symbolic). See README.md.
//
//   bun tools/registry-diff/diff.ts [--only <scenario,...>] [--corpus <chains per format>] [--seed n] [--out <dir>]
//
// Targets: REGDIFF_REF (default http://127.0.0.1:40081, Confluent 7.7.1), REGDIFF_REF8 (http://127.0.0.1:40082, 8.x; set it
// to "" to skip), REGDIFF_DOZE (http://127.0.0.1:40011). The report says, step by step, where doze differs from 7.7.1 and
// where 7.7.1 and 8.x differ from each other. Exit 1 when doze differs from the reference anywhere not listed in known.ts.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { comparable, type Exchange, Ids, reset, send, type Step, type Target } from "./lib.ts";
import { scenarios } from "./scenarios.ts";
import { corpus } from "./corpus.ts";
import { known } from "./known.ts";
import { gaps } from "./gaps.ts";

const args = process.argv.slice(2);
const opt = (name: string, dflt: string) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : dflt;
};
const only = opt("--only", "")
  .split(",")
  .filter((s) => s);
const chains = Number(opt("--corpus", "0"));
const seed = Number(opt("--seed", "1"));
const outDir = opt("--out", join(import.meta.dir, "results", "latest"));

const ref: Target = { name: "confluent-7.7.1", url: process.env.REGDIFF_REF ?? "http://127.0.0.1:40081" };
const ref8url = process.env.REGDIFF_REF8 ?? "http://127.0.0.1:40082";
const ref8: Target | null = ref8url ? { name: "confluent-8", url: ref8url } : null;
const doze: Target = { name: "doze", url: process.env.REGDIFF_DOZE ?? "http://127.0.0.1:40011" };
const targets = [ref, ...(ref8 ? [ref8] : []), doze];

type Diff = { scenario: string; step: number; req: string; known?: string; a: unknown; b: unknown };

async function run(name: string, steps: Step[]) {
  const ids = new Map(targets.map((t) => [t.name, new Ids()]));
  const exchanges: Exchange[] = [];
  await Promise.all(targets.map(reset));
  for (const step of steps) {
    const answers = Object.fromEntries(
      await Promise.all(targets.map(async (t) => [t.name, await send(t, ids.get(t.name)!, step)] as const)),
    );
    exchanges.push({ step, answers });
  }
  const diffs = (x: Target, y: Target): Diff[] => {
    const out: Diff[] = [];
    exchanges.forEach((e, i) => {
      const a = comparable(e.answers[x.name], e.step.p, ids.get(x.name)!);
      const b = comparable(e.answers[y.name], e.step.p, ids.get(y.name)!);
      if (JSON.stringify(a) !== JSON.stringify(b)) {
        const key = `${name}/${i}`;
        out.push({ scenario: name, step: i, req: `${e.step.m} ${e.step.p}`, known: known[key] ?? known[`${name}/*`], a, b });
      }
    });
    return out;
  };
  return { exchanges, dozeVsRef: diffs(ref, doze), ref8VsRef: ref8 ? diffs(ref, ref8) : [], dozeVsRef8: ref8 ? diffs(ref8, doze) : [] };
}

const all: Record<string, Step[]> = { ...scenarios, ...gaps };
if (chains > 0) Object.assign(all, corpus(chains, seed));
const names = Object.keys(all).filter((n) => only.length === 0 || only.some((o) => n === o || n.startsWith(o + "/")));

mkdirSync(outDir, { recursive: true });
const summary: { scenario: string; steps: number; dozeVsRef: number; unknown: number; ref8VsRef: number; dozeVsRef8: number }[] = [];
const allDiffs: Record<string, Diff[]> = { dozeVsRef: [], ref8VsRef: [], dozeVsRef8: [] };
for (const n of names) {
  const r = await run(n, all[n]);
  const unknown = r.dozeVsRef.filter((d) => !d.known).length;
  summary.push({ scenario: n, steps: all[n].length, dozeVsRef: r.dozeVsRef.length, unknown, ref8VsRef: r.ref8VsRef.length, dozeVsRef8: r.dozeVsRef8.length });
  allDiffs.dozeVsRef.push(...r.dozeVsRef);
  allDiffs.ref8VsRef.push(...r.ref8VsRef);
  allDiffs.dozeVsRef8.push(...r.dozeVsRef8);
  console.log(
    `${unknown === 0 ? "ok  " : "DIFF"} ${n}: ${all[n].length} steps, doze vs 7.7.1 ${r.dozeVsRef.length} (${unknown} unexplained), 8 vs 7.7.1 ${r.ref8VsRef.length}`,
  );
  const safe = n.replace(/[^a-zA-Z0-9.-]+/g, "_");
  if (!n.startsWith("corpus/")) writeFileSync(join(outDir, `${safe}.exchanges.json`), JSON.stringify(r.exchanges, null, 1));
}
writeFileSync(join(outDir, "summary.json"), JSON.stringify(summary, null, 1));
for (const [k, v] of Object.entries(allDiffs)) writeFileSync(join(outDir, `${k}.json`), JSON.stringify(v, null, 1));
const unexplained = allDiffs.dozeVsRef.filter((d) => !d.known);
console.log(
  `\n${names.length} scenarios, ${summary.reduce((s, x) => s + x.steps, 0)} steps: doze differs from 7.7.1 at ${allDiffs.dozeVsRef.length} (${unexplained.length} unexplained); 8.x differs from 7.7.1 at ${allDiffs.ref8VsRef.length}; results in ${outDir}`,
);
process.exit(unexplained.length === 0 ? 0 : 1);
