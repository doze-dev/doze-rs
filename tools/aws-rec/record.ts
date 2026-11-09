#!/usr/bin/env bun
// Records conformance modules against AWS one at a time, sweeps the account after each, and prints one summary line per module.
//   bun tools/aws-rec/record.ts sqs ssm sns      (module names without the test_ prefix)
//   bun tools/aws-rec/record.ts s3:presigned     (module:keyword re-records only scenarios matching the pytest -k expression)
// Needs a live SSO session: aws sso login --profile doze-rec-main
import { assertRecorder, EXPECT_ACCOUNT, PROFILE } from "./lib.ts";

const UV = process.env.UV_BIN ?? `${process.env.HOME}/.proto/tools/uv/0.12.19/uv`;
const here = import.meta.dir;
const conformance = `${here}/../../conformance`;
const modules = process.argv.slice(2);
if (!modules.length) { console.error("usage: record.ts <module> [<module> ...]"); process.exit(2); }
console.log(`recorder: ${await assertRecorder()}`);

async function run(cmd: string[], cwd: string, env: Record<string, string | undefined> = {}) {
  const p = Bun.spawn(cmd, { cwd, stdout: "pipe", stderr: "pipe", env: { ...process.env, ...env } });
  const [out, err] = await Promise.all([new Response(p.stdout).text(), new Response(p.stderr).text()]);
  return { code: await p.exited, text: out + err };
}

const rows: string[] = [];
for (const spec of modules) {
  const [m, k] = spec.split(":");
  const started = Date.now();
  const env = { AWS_PROFILE: PROFILE, CONFORMANCE_TARGET: "aws", CONFORMANCE_AWS_ACCOUNT: EXPECT_ACCOUNT, CONFORMANCE_RECORD: "1", CONFORMANCE_ENDPOINT: undefined };
  const r = await run([UV, "run", "--frozen", "pytest", `tests/test_${m}.py`, "-p", "no:cacheprovider", ...(k ? ["-k", k] : [])], conformance, env as Record<string, string>);
  await Bun.write(`${here}/logs/record-${m}${k ? "-k" : ""}.log`, r.text);
  const summary = r.text.trim().split("\n").filter((l) => /passed|failed|error/.test(l)).pop() ?? "no summary";
  const rec = r.text.match(/recorded (\d+) responses/)?.[1] ?? "0";
  const sweep = await run(["bun", `${here}/teardown.ts`, "--delete"], here);
  const clean = /CLEAN: nothing/.test(sweep.text) ? "clean" : "RESIDUE";
  rows.push(`${spec.padEnd(24)} recorded ${rec.padStart(4)}  ${summary.replace(/ in [\d.]+s.*/, "").padEnd(34)} ${clean}  ${Math.round((Date.now() - started) / 1000)}s`);
  console.log(rows[rows.length - 1]);
  if (clean !== "clean") await Bun.write(`${here}/logs/sweep-${m}.log`, sweep.text);
}
console.log("\n" + rows.join("\n"));
