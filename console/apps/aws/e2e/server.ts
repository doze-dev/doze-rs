// A real doze-aws for the browser tests: started on ports of its own with an empty data directory, called with the real AWS
// CLI, and stopped by its pid. Nothing here is a fixture: the pages under test read what this process answers.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { closeSync, existsSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = join(import.meta.dirname, "../../../..");

/** The binary under test: `DOZE_AWS_BIN`, else the debug build in `CARGO_TARGET_DIR` (or the repo's `target/`). */
export function binary(): string {
  const bin = process.env.DOZE_AWS_BIN ?? join(process.env.CARGO_TARGET_DIR ?? join(repo, "target"), "debug", process.platform === "win32" ? "doze-aws.exe" : "doze-aws");
  if (!existsSync(bin)) throw new Error(`no doze-aws at ${bin}: run \`bun run build\` in console/apps/aws, then \`cargo build -p doze-aws\` (or set DOZE_AWS_BIN)`);
  return bin;
}

export type Running = { pid: number; endpoint: string; console: string; dataDir: string; account: string; stop(): Promise<void> };

export const ACCOUNT = "111122223333";

async function until(what: string, probe: () => Promise<boolean>, ms = 15_000): Promise<void> {
  const end = Date.now() + ms;
  for (;;) {
    if (await probe().catch(() => false)) return;
    if (Date.now() > end) throw new Error(`${what} did not happen in ${ms} ms`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

/** How long one start may take before it is tried again, and how many tries (a loaded CI runner has been seen not to answer
 * within 15 s once; the output of every try is kept so a hang is diagnosable). */
const START_MS = 30_000;
const START_TRIES = 2;

/** Starts doze-aws with its console on the two ports and waits until both answer. `dataDir`: start again over the same
 * state (a restart); otherwise a new, empty directory that `stop` removes. A start that never answers is killed by its pid and
 * tried once more; when the last try fails the error carries what the process printed. */
export async function start(listenPort: number, consolePort: number, options: { dataDir?: string; args?: string[] } = {}): Promise<Running> {
  const own = options.dataDir === undefined;
  const dataDir = options.dataDir ?? mkdtempSync(join(tmpdir(), "doze-aws-e2e-"));
  const endpoint = `http://127.0.0.1:${listenPort}`;
  const consoleUrl = `http://127.0.0.1:${consolePort}`;
  const argv = ["--listen", `127.0.0.1:${listenPort}`, "--console", `127.0.0.1:${consolePort}`, "--data-dir", dataDir, "--account", ACCOUNT, ...(options.args ?? [])];
  // Something already answering on the console port is another process: its answers would be taken for this one's.
  if (await fetch(`${consoleUrl}/healthz`, { signal: AbortSignal.timeout(1_000) }).then(() => true, () => false)) throw new Error(`${consoleUrl} is already served by another process: pick free ports`);
  const notes: string[] = [];
  for (let attempt = 1; attempt <= START_TRIES; attempt++) {
    const logPath = join(mkdtempSync(join(tmpdir(), "doze-aws-e2e-log-")), "out.log");
    const fd = openSync(logPath, "w");
    const child: ChildProcess = spawn(binary(), argv, { stdio: ["ignore", fd, fd] });
    closeSync(fd);
    const pid = child.pid;
    if (pid === undefined) throw new Error("doze-aws did not start");
    let exited = false;
    child.once("exit", () => (exited = true));
    const began = Date.now();
    try {
      await ready(consoleUrl, START_MS, () => (exited ? `it exited at start (are ports ${listenPort} and ${consolePort} free?)` : undefined));
    } catch (e) {
      notes.push(`try ${attempt} (${Date.now() - began} ms, ${exited ? "exited" : "still running"}): ${(e as Error).message}; it printed: ${tail(logPath)}`);
      if (!exited) {
        process.kill(pid, "SIGKILL");
        await until("doze-aws killed", async () => exited, 5_000).catch(() => undefined);
      }
      continue;
    }
    return {
      pid,
      endpoint,
      console: consoleUrl,
      dataDir,
      account: ACCOUNT,
      async stop() {
        // By its pid, never by its name: other doze-aws processes on this machine are not this test's.
        if (!exited) {
          process.kill(pid, "SIGTERM");
          await until("doze-aws stopping", async () => exited, 10_000).catch(() => process.kill(pid, "SIGKILL"));
        }
        if (own) rmSync(dataDir, { recursive: true, force: true });
      },
    };
  }
  if (own) rmSync(dataDir, { recursive: true, force: true });
  throw new Error(`doze-aws on ${consoleUrl} did not start in ${START_TRIES} tries:\n${notes.join("\n")}`);
}

/** Polls the console's /healthz until it answers; a probe that fails is only "not yet", but `gone` (the process is dead) ends the wait at once. */
async function ready(consoleUrl: string, ms: number, gone: () => string | undefined): Promise<void> {
  const end = Date.now() + ms;
  for (;;) {
    const why = gone();
    if (why !== undefined) throw new Error(why);
    if (await fetch(`${consoleUrl}/healthz`, { signal: AbortSignal.timeout(2_000) }).then((r) => r.ok, () => false)) return;
    if (Date.now() > end) throw new Error(`no answer from ${consoleUrl}/healthz in ${ms} ms`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

/** The last 1,500 bytes of what the process printed (its log is the only evidence a hung start leaves). */
function tail(path: string): string {
  try {
    const text = readFileSync(path, "utf8");
    return text.length === 0 ? "(nothing)" : text.slice(-1_500);
  } catch {
    return "(no log)";
  }
}

/** Fails when the binary serves another bundle than the one in web-dist (a binary built before the last `bun run build`),
 * or the placeholder: the tests would be looking at old pages. */
export async function assertCurrentBundle(consoleUrl: string): Promise<void> {
  const served = await (await fetch(`${consoleUrl}/`)).text();
  if (served.includes("is not built yet")) throw new Error("this doze-aws serves the placeholder page: run `bun run build` in console/apps/aws, then `cargo build -p doze-aws`");
  const built = readFileSync(join(repo, "crates/aws-console/web-dist/index.html"), "utf8");
  const scripts = (html: string) => [...html.matchAll(/assets\/[\w.-]+\.(?:js|css)/g)].map((m) => m[0]).sort();
  if (scripts(served).join() !== scripts(built).join()) throw new Error(`this doze-aws was built before the bundle in web-dist (it serves ${scripts(served).join(", ")}): run \`cargo build -p doze-aws\``);
}

/** One call with the real AWS CLI against `endpoint`, signed with dummy credentials (doze-aws trusts any key by default).
 * Returns its exit code and what it printed; a call that fails is not thrown, it is a result. */
export function aws(endpoint: string, args: string[], options: { accessKey?: string; region?: string } = {}): { code: number; out: string } {
  const env = { ...process.env, AWS_ENDPOINT_URL: endpoint, AWS_ACCESS_KEY_ID: options.accessKey ?? "AKIADOZEE2E00000001", AWS_SECRET_ACCESS_KEY: "dummy", AWS_DEFAULT_REGION: options.region ?? "us-east-1", AWS_PAGER: "", AWS_EC2_METADATA_DISABLED: "true", AWS_MAX_ATTEMPTS: "1" };
  delete (env as Record<string, string | undefined>).AWS_PROFILE;
  delete (env as Record<string, string | undefined>).AWS_SESSION_TOKEN;
  try {
    return { code: 0, out: execFileSync("aws", args, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 120_000 }) };
  } catch (e) {
    const err = e as { status?: number; stdout?: string; stderr?: string; message: string };
    if (err.status === undefined || err.status === null) throw new Error(`the AWS CLI could not be run: ${err.message}`, { cause: e });
    return { code: err.status, out: `${err.stdout ?? ""}${err.stderr ?? ""}` };
  }
}
