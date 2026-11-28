// loop-probe: how long a cheap call waits while something heavy runs.
//
// doze-aws answers every service on one event loop (crates/doze-aws/src/run.rs). Work that blocks it (key
// generation, hashing, fsync, a directory delete) shows up as latency on unrelated calls. This probe fires an SQS
// ListQueues (signed with SigV4, the credentials doze-aws accepts) every few milliseconds, one at a time, and prints
// p50/p99/max of the round trip. Start it, run the heavy operation in another shell, stop it with Ctrl-C (or give
// --seconds) and read the numbers. It starts no server.
//
//   bun tools/loop-probe.ts --endpoint http://127.0.0.1:4630 [--seconds 20] [--interval-ms 10]
//
// Every second it prints the window's numbers; on exit the whole run's.
import { createHash, createHmac } from "node:crypto";

function arg(name: string, fallback: string): string {
  const at = process.argv.indexOf(`--${name}`);
  return at >= 0 && at + 1 < process.argv.length ? process.argv[at + 1] : fallback;
}

const endpoint = arg("endpoint", "http://127.0.0.1:4630");
const seconds = Number(arg("seconds", "0"));
const intervalMs = Number(arg("interval-ms", "10"));
const region = arg("region", "us-east-1");
const accessKey = process.env.AWS_ACCESS_KEY_ID ?? "test";
const secretKey = process.env.AWS_SECRET_ACCESS_KEY ?? "test";

const sha256 = (data: string) => createHash("sha256").update(data).digest("hex");
const hmac = (key: string | Buffer, data: string) => createHmac("sha256", key).update(data).digest();

// One SigV4-signed SQS ListQueues (JSON protocol).
function signed(): { headers: Record<string, string>; body: string } {
  const url = new URL(endpoint);
  const body = "{}";
  const now = new Date().toISOString().replace(/[:-]|\.\d{3}/g, "");
  const day = now.slice(0, 8);
  const headers: Record<string, string> = {
    "content-type": "application/x-amz-json-1.0",
    host: url.host,
    "x-amz-date": now,
    "x-amz-target": "AmazonSQS.ListQueues",
  };
  const names = Object.keys(headers).sort();
  const canonical = [
    "POST",
    "/",
    "",
    names.map((n) => `${n}:${headers[n]}\n`).join(""),
    names.join(";"),
    sha256(body),
  ].join("\n");
  const scope = `${day}/${region}/sqs/aws4_request`;
  const toSign = ["AWS4-HMAC-SHA256", now, scope, sha256(canonical)].join("\n");
  let key = hmac(`AWS4${secretKey}`, day);
  for (const part of [region, "sqs", "aws4_request"]) key = hmac(key, part);
  const signature = createHmac("sha256", key).update(toSign).digest("hex");
  headers.authorization = `AWS4-HMAC-SHA256 Credential=${accessKey}/${scope}, SignedHeaders=${names.join(";")}, Signature=${signature}`;
  return { headers, body };
}

function summary(samples: number[]): string {
  if (samples.length === 0) return "no samples";
  const sorted = [...samples].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  return `n=${sorted.length} p50=${at(0.5).toFixed(1)}ms p99=${at(0.99).toFixed(1)}ms max=${sorted[sorted.length - 1].toFixed(1)}ms`;
}

const all: number[] = [];
let window: number[] = [];
let failures = 0;
let stop = false;

function finish() {
  stop = true;
  console.log(`total ${summary(all)} failures=${failures}`);
  process.exit(0);
}
process.on("SIGINT", finish);
if (seconds > 0) setTimeout(finish, seconds * 1000);

setInterval(() => {
  console.log(`${new Date().toISOString().slice(11, 19)} ${summary(window)}`);
  window = [];
}, 1000);

while (!stop) {
  const { headers, body } = signed();
  const started = performance.now();
  try {
    const response = await fetch(endpoint, { method: "POST", headers, body });
    await response.arrayBuffer();
    if (!response.ok) failures++;
  } catch {
    failures++;
  }
  const took = performance.now() - started;
  all.push(took);
  window.push(took);
  await Bun.sleep(intervalMs);
}
