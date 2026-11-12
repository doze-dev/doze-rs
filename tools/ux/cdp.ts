// A small Chrome DevTools Protocol client for the interaction tests (tools/ux/drive.ts). Bun's WebSocket, no packages.
//
// Rules it enforces (docs/port/console/ux-tests.md): ONE Chrome at a time (it refuses to start when one is running), a fresh
// profile directory per run, a free debugging port, and it kills only the process it started.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

type Json = Record<string, unknown>;
type Pending = { resolve: (v: Json) => void; reject: (e: Error) => void };

/** One browser, started by us. */
export class Browser {
  private ws!: WebSocket;
  private next = 1;
  private pending = new Map<number, Pending>();
  private handlers: Array<(method: string, params: Json, sessionId?: string) => void> = [];
  private proc!: ReturnType<typeof Bun.spawn>;
  private dir = "";

  static async launch(chrome = DEFAULT_CHROME, size = { width: 1280, height: 800 }): Promise<Browser> {
    // A crashpad handler may linger after a Chrome has gone: it is not a browser.
    const running = Bun.spawnSync(["pgrep", "-fl", "Google Chrome"]).stdout.toString().split("\n").filter((l) => l.trim() && !l.includes("chrome_crashpad_handler")).join("\n");
    if (running) throw new Error(`a Chrome is already running; one at a time:\n${running}`);
    const b = new Browser();
    b.dir = mkdtempSync(join(tmpdir(), "ux-chrome-"));
    const port = 20000 + Math.floor(Math.random() * 20000);
    b.proc = Bun.spawn(
      [chrome, "--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${b.dir}`, "--no-first-run", "--no-default-browser-check",
        "--disable-gpu", "--hide-scrollbars=false", `--window-size=${size.width},${size.height}`, "--force-device-scale-factor=1", "about:blank"],
      { stdout: "ignore", stderr: "ignore" },
    );
    let url = "";
    for (let i = 0; i < 100 && !url; i++) {
      await Bun.sleep(100);
      try {
        const r = await fetch(`http://127.0.0.1:${port}/json/version`);
        url = ((await r.json()) as { webSocketDebuggerUrl: string }).webSocketDebuggerUrl;
      } catch { /* not up yet */ }
    }
    if (!url) { b.kill(); throw new Error("Chrome did not start"); }
    b.ws = new WebSocket(url);
    await new Promise<void>((res, rej) => { b.ws.onopen = () => res(); b.ws.onerror = () => rej(new Error("websocket")); });
    b.ws.onmessage = (m) => {
      const j = JSON.parse(String(m.data)) as { id?: number; result?: Json; error?: { message: string }; method?: string; params?: Json; sessionId?: string };
      if (j.id !== undefined) {
        const p = b.pending.get(j.id);
        if (!p) return;
        b.pending.delete(j.id);
        if (j.error) p.reject(new Error(j.error.message)); else p.resolve(j.result ?? {});
      } else if (j.method) for (const h of b.handlers) h(j.method, j.params ?? {}, j.sessionId);
    };
    return b;
  }

  send(method: string, params: Json = {}, sessionId?: string): Promise<Json> {
    const id = this.next++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }

  on(h: (method: string, params: Json, sessionId?: string) => void): void { this.handlers.push(h); }

  /** A fresh tab with the domains the tests use enabled. */
  async tab(): Promise<{ sessionId: string; targetId: string }> {
    const { targetId } = (await this.send("Target.createTarget", { url: "about:blank" })) as { targetId: string };
    const { sessionId } = (await this.send("Target.attachToTarget", { targetId, flatten: true })) as { sessionId: string };
    for (const d of ["Page", "Runtime", "Log", "Network"]) await this.send(`${d}.enable`, {}, sessionId);
    return { sessionId, targetId };
  }

  kill(): void {
    try { this.ws?.close(); } catch { /* closed */ }
    try { this.proc?.kill(); } catch { /* gone */ }
    try { rmSync(this.dir, { recursive: true, force: true }); } catch { /* gone */ }
  }

  /** The pid of the browser we started, so a lingering helper can be found by hand. */
  get pid(): number { return this.proc.pid; }
}
