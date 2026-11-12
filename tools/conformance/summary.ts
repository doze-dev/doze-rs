#!/usr/bin/env bun
// Runs one conformance module against the Rust doze-aws and says, per scenario, why it is not green.
//   bun tools/conformance/summary.ts sqs [pytest -k expression]
// Needs target/debug/doze-aws (cargo build -p doze-aws) and uv (UV_BIN, default ~/.proto/tools/uv/0.12.19/uv).
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:net";

const root = `${import.meta.dir}/../..`;
const module_ = process.argv[2] ?? "sqs";
const keyword = process.argv[3];
const UV = process.env.UV_BIN ?? `${process.env.HOME}/.proto/tools/uv/0.12.19/uv`;
const BIN = process.env.DOZE_AWS ?? `${root}/target/debug/doze-aws`;

const port = await new Promise<number>((res) => { const s = createServer(); s.listen(0, "127.0.0.1", () => { const p = (s.address() as { port: number }).port; s.close(() => res(p)); }); });
const data = mkdtempSync(join(tmpdir(), "dz-summary-"));
const server = Bun.spawn([BIN, "--listen", `127.0.0.1:${port}`, "--data-dir", data], { cwd: data, stdout: "ignore", stderr: "ignore" });
await Bun.sleep(800);
const args = ["run", "--frozen", "pytest", `tests/test_${module_}.py`, "-p", "no:cacheprovider", "-q", ...(keyword ? ["-k", keyword] : [])];
const p = Bun.spawn([UV, ...args], { cwd: `${root}/conformance`, stdout: "pipe", stderr: "pipe", env: { ...process.env, CONFORMANCE_ENDPOINT: `http://127.0.0.1:${port}`, CONFORMANCE_TARGET: "doze" } });
const text = (await new Response(p.stdout).text()) + (await new Response(p.stderr).text());
await p.exited;
server.kill();

// Sections: "_____ ERROR at teardown of test_x _____" (labels differ) and "_____ test_x _____" (the scenario stopped).
const sections = text.split(/\n(?=_{3,} (?:ERROR at teardown of )?test_\w+ _{3,}\n)/);
const green = (text.match(/(\d+) passed/) ?? [])[1] ?? "0";
console.log(`${green} scenarios fully green`);
for (const s of sections.slice(1)) {
  const head = s.match(/^_{3,} (ERROR at teardown of )?(test_\w+)/);
  if (!head) continue;
  const name = head[2];
  if (head[1]) {
    const labels = [...s.matchAll(/^E {13}\[(.+)\]$/gm)].map((m) => m[1]);
    console.log(`DIFF  ${name}: ${[...new Set(labels)].join(", ")}`);
  } else {
    const err = s.split("\n").reverse().find((l) => /^E /.test(l)) ?? "";
    console.log(`STOP  ${name}: ${err.slice(2, 170)}`);
  }
}
