// A real doze-aws for the browser tests: started on ports of its own with an empty data directory, called with the real AWS
// CLI, and stopped by its pid. Nothing here is a fixture: the pages under test read what this process answers.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
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

/** Starts doze-aws with its console on the two ports and waits until both answer. `dataDir`: start again over the same
 * state (a restart); otherwise a new, empty directory that `stop` removes. */
export async function start(listenPort: number, consolePort: number, options: { dataDir?: string; args?: string[] } = {}): Promise<Running> {
  const own = options.dataDir === undefined;
  const dataDir = options.dataDir ?? mkdtempSync(join(tmpdir(), "doze-aws-e2e-"));
  const endpoint = `http://127.0.0.1:${listenPort}`;
  const consoleUrl = `http://127.0.0.1:${consolePort}`;
  const child: ChildProcess = spawn(binary(), ["--listen", `127.0.0.1:${listenPort}`, "--console", `127.0.0.1:${consolePort}`, "--data-dir", dataDir, "--account", ACCOUNT, ...(options.args ?? [])], { stdio: "ignore" });
  const pid = child.pid;
  if (pid === undefined) throw new Error("doze-aws did not start");
  let exited = false;
  child.once("exit", () => (exited = true));
  await until(`doze-aws on ${consoleUrl}`, async () => {
    if (exited) throw new Error(`doze-aws exited at start (are ports ${listenPort} and ${consolePort} free?)`);
    return (await fetch(`${consoleUrl}/healthz`)).ok;
  });
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
