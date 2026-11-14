// Go baselines for the core plan's weight.md: daemon idle RSS and threads, status latency, wake latency.
// bun baseline.ts <doze> <hello-server>. Uses a throwaway DOZE_HOME under /tmp (unix socket path limit).
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

const [doze, hello] = process.argv.slice(2);
const home = fs.mkdtempSync("/tmp/dzb-");
const dir = path.join(home, "stack");
fs.mkdirSync(dir);
const ports = [0, 1, 2].map(() => 20000 + Math.floor(Math.random() * 20000));
const svc = ports.map((p, i) => `service "s${i}" {\n  run   = "${hello}"\n  port  = ${p}\n  sleep = "2s"\n}\n`).join("\n");
fs.writeFileSync(path.join(dir, "doze.hcl"), `stack = "probe"\n\ndefaults {\n  names = false\n}\n\n${svc}`);
const env = { ...process.env, DOZE_HOME: home, HOME: home, NO_COLOR: "1" };
const run = (...a: string[]) => {
  const t = performance.now();
  const r = Bun.spawnSync([doze, ...a], { cwd: dir, env });
  return { ms: performance.now() - t, code: r.exitCode, out: r.stdout.toString() + r.stderr.toString() };
};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const median = (xs: number[]) => xs.sort((a, b) => a - b)[Math.floor(xs.length / 2)];

function wakeOnce(port: number): Promise<number> {
  return new Promise((resolve, reject) => {
    const t = performance.now();
    const s = net.connect(port, "127.0.0.1", () => s.write("GET / HTTP/1.0\r\n\r\n"));
    s.once("data", () => { resolve(performance.now() - t); s.destroy(); });
    s.once("error", reject);
  });
}

try {
  const up = run("-y", "up");
  if (up.code !== 0) throw new Error("up: " + up.out);
  await sleep(6000); // everything asleep (sleep = 2s)
  const pidFile = fs.readdirSync(path.join(home, "projects")).map((p) => path.join(home, "projects", p, "run", "doze.pid")).find((f) => fs.existsSync(f))!;
  const pid = fs.readFileSync(pidFile, "utf8").trim();
  const rss = Number(Bun.spawnSync(["ps", "-o", "rss=", "-p", pid]).stdout.toString().trim());
  const threads = Bun.spawnSync(["ps", "-M", "-p", pid]).stdout.toString().trim().split("\n").length - 1;
  const status = [] as number[];
  for (let i = 0; i < 15; i++) status.push(run("status").ms);
  const version = [] as number[];
  for (let i = 0; i < 15; i++) version.push(run("version").ms);
  const wakes = [] as number[];
  for (let i = 0; i < 5; i++) {
    wakes.push(await wakeOnce(ports[0]));
    await sleep(5000);
  }
  const warm = [] as number[];
  for (let i = 0; i < 10; i++) warm.push(await wakeOnce(ports[1]));
  console.log(JSON.stringify({
    os: `${os.platform()}/${os.arch()}`, daemon_idle_rss_kb: rss, daemon_threads: threads,
    status_ms_median: +median(status).toFixed(1), version_ms_median: +median(version).toFixed(1),
    cold_wake_ms: wakes.map((w) => +w.toFixed(1)), warm_connect_ms_median: +median(warm).toFixed(2),
  }));
} finally {
  run("-y", "down");
  fs.rmSync(home, { recursive: true, force: true });
}
