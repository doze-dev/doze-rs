// Starts the real binary (target/release/doze-kafka, or DOZE_KAFKA_BIN) on a fresh data directory and seeds a small world
// through the Kafka protocol (seed-world.ts): three topics, records in them, a consumer group with committed offsets. Returns
// the teardown, which stops only the process it started.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { SEEDED } from "./playwright.config";
import { seedWorld } from "./seed-world";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function waitFor(url: string, ms: number) {
  const end = Date.now() + ms;
  for (;;) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {
      // not yet
    }
    if (Date.now() > end) throw new Error(`${url} did not answer in ${ms} ms`);
    await sleep(100);
  }
}

export default async function setup() {
  const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../target/release/doze-kafka");
  const data = mkdtempSync(join(tmpdir(), "doze-e2e-"));
  const child = spawn(bin, ["--listen", SEEDED.kafka, "--data-dir", data, "--console-addr", SEEDED.console.replace("http://", ""), "--schema-registry-addr", SEEDED.registry], { stdio: "ignore" });
  await waitFor(`${SEEDED.console}/healthz`, 15_000);
  await seedWorld();

  return async () => {
    child.kill("SIGTERM");
    await new Promise((r) => child.once("exit", r));
    rmSync(data, { recursive: true, force: true });
  };
}
