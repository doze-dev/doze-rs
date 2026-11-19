// Runs Go's oracle-dump over the folders in ../oracle/multi (each a stack loaded as a folder) and
// writes go-multi.jsonl and go-multi-shape.jsonl, with local paths masked as in README.md.
//
//   bun multi.ts <oracle-dump> <fresh empty scratch home>
import { readdirSync, realpathSync, statSync } from "node:fs";
import { join } from "node:path";

const [dump, home] = Bun.argv.slice(2);
if (!dump || !home) throw new Error("usage: bun multi.ts <oracle-dump> <scratch home>");
const oracle = join(import.meta.dir, "../oracle");
const multi = realpathSync(join(oracle, "multi"));
const cases = readdirSync(multi).filter((n) => statSync(join(multi, n)).isDirectory()).sort();
const realHome = realpathSync(home);

function mask(s: string): string {
  for (const h of [realHome, realHome.replace(/^\/private/, ""), home]) s = s.replaceAll(h, "<home>");
  for (const d of [multi, multi.replace(/^\/private/, "")]) s = s.replaceAll(d, "<dir>");
  return s.replace(/multi-[0-9a-f]{8}/g, "multi-<slug>");
}

for (const [mode, out] of [["", "go-multi.jsonl"], ["-shape", "go-multi-shape.jsonl"]]) {
  const args = ["config", ...(mode ? [mode] : []), ...cases];
  const p = Bun.spawnSync([dump, ...args], { cwd: multi, env: { PATH: "/usr/bin:/bin", HOME: home } });
  await Bun.write(join(oracle, out), mask(p.stdout.toString()));
}
