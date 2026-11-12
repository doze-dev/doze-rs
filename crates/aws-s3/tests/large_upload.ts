// The flat-memory gate for large objects: streams SIZE_MIB of data into S3 and back out through a release doze-aws, sampling the
// server's resident memory the whole time, and fails if it grew by more than LIMIT_MIB over where it started.
//
//   bun crates/aws-s3/tests/large_upload.ts target/release-aws/doze-aws [size_mib=1024] [limit_mib=64]
//
// The data is generated as it is sent and hashed as it comes back, so neither the client nor the server holds the object.

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [bin, sizeArg = "1024", limitArg = "64"] = process.argv.slice(2);
if (!bin) {
  console.error("usage: bun large_upload.ts <doze-aws binary> [size_mib] [limit_mib]");
  process.exit(2);
}
const sizeMib = Number(sizeArg);
const limitMib = Number(limitArg);
const port = 20000 + Math.floor(Math.random() * 20000);
const dir = mkdtempSync(join(tmpdir(), "s3-large-"));
const server = spawn(bin, ["--listen", `127.0.0.1:${port}`, "--data-dir", dir, "--account", "111122223333"], { stdio: "ignore" });

const auth =
  "AWS4-HMAC-SHA256 Credential=test/20260101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=00";
const headers = { Authorization: auth, "x-amz-date": "20260101T000000Z", "x-amz-content-sha256": "UNSIGNED-PAYLOAD" };
const base = `http://127.0.0.1:${port}`;

async function rssMib(): Promise<number> {
  const p = Bun.spawn(["ps", "-o", "rss=", "-p", String(server.pid)]);
  return Number((await new Response(p.stdout).text()).trim()) / 1024;
}

async function waitUp() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(base, { headers });
      if (r.status) return;
    } catch {
      await Bun.sleep(50);
    }
  }
  throw new Error("the server did not start");
}

let peak = 0;
const sampler = setInterval(async () => {
  peak = Math.max(peak, await rssMib());
}, 50);

try {
  await waitUp();
  const created = await fetch(`${base}/large`, { method: "PUT", headers });
  if (created.status !== 200) throw new Error(`create bucket: ${created.status}`);
  const start = await rssMib();
  peak = start;

  const piece = new Uint8Array(1024 * 1024).map((_, i) => (i * 31) & 255);
  const sent = new Bun.CryptoHasher("sha256");
  let n = 0;
  const body = new ReadableStream({
    pull(controller) {
      if (n >= sizeMib) return controller.close();
      piece[0] = n & 255;
      sent.update(piece);
      controller.enqueue(piece.slice());
      n++;
    },
  });
  const t0 = performance.now();
  const put = await fetch(`${base}/large/object`, {
    method: "PUT",
    headers: { ...headers, "content-length": String(sizeMib * 1024 * 1024) },
    body,
    // @ts-expect-error Bun streams a request body this way
    duplex: "half",
  });
  if (put.status !== 200) throw new Error(`put: ${put.status} ${await put.text()}`);
  const t1 = performance.now();

  const got = await fetch(`${base}/large/object`, { headers });
  const back = new Bun.CryptoHasher("sha256");
  let bytes = 0;
  for await (const chunk of got.body!) {
    back.update(chunk);
    bytes += chunk.length;
  }
  const t2 = performance.now();
  clearInterval(sampler);
  peak = Math.max(peak, await rssMib());
  const grew = peak - start;
  const ok = bytes === sizeMib * 1024 * 1024 && sent.digest("hex") === back.digest("hex") && grew <= limitMib;
  console.log(
    `${sizeMib} MiB up in ${((t1 - t0) / 1000).toFixed(1)} s, down in ${((t2 - t1) / 1000).toFixed(1)} s; server RSS ${start.toFixed(1)} MiB at start, peak ${peak.toFixed(1)} MiB (grew ${grew.toFixed(1)}, limit ${limitMib}); bytes ${bytes}; ${ok ? "PASS" : "FAIL"}`,
  );
  process.exitCode = ok ? 0 : 1;
} finally {
  clearInterval(sampler);
  server.kill();
  rmSync(dir, { recursive: true, force: true });
}
