// A small Chrome DevTools Protocol client for the console's page scenarios (plain bun, no packages). Not in CI: it drives a real
// Chrome, one at a time, under the lock the whole team shares.
//
//   const b = await Browser.launch();           // takes the lock, checks no other Chrome runs, starts headless Chrome
//   const p = await b.page();                   // a tab with a layout-shift observer and console capture installed
//   await p.goto("http://127.0.0.1:19412/");
//   ...
//   await b.close();                            // kills OUR Chrome (by pid), removes the profile, releases the lock
//
// Rules it enforces: one Chrome at a time (lock directory + `pgrep -fl "Google Chrome"` must be empty), a fresh
// --user-data-dir, our own pid killed and nobody else's, the lock released even on failure, held no longer than 3 minutes.

import { mkdirSync, mkdtempSync, rmSync, rmdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const LOCK = "/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/chrome.lock";
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const chromeProcesses = () => Bun.spawnSync(["pgrep", "-fl", "Google Chrome"]).stdout.toString().split("\n").filter((l) => l.trim() && !l.includes("chrome_crashpad_handler")).length;

export class Browser {
  private constructor(
    private proc: ReturnType<typeof Bun.spawn>,
    private profile: string,
    private port: number,
    private started: number,
    private timer: ReturnType<typeof setTimeout>,
  ) {}

  static async launch(opts: { width?: number; height?: number } = {}, attempt = 0): Promise<Browser> {
    // The lock: mkdir is atomic. Wait up to 10 minutes for another agent's Chrome.
    for (let i = 0; ; i++) {
      try {
        mkdirSync(LOCK);
        break;
      } catch {
        if (i > 60) throw new Error("the Chrome lock is held; giving up");
        await sleep(10_000);
      }
    }
    try {
      if (chromeProcesses() !== 0) {
        // Somebody else's Chrome (not under our lock): let go of the lock, wait, and try again.
        rmdirSync(LOCK);
        if (attempt > 40) throw new Error("a Chrome is still running; refusing to start another");
        await sleep(15_000);
        return Browser.launch(opts, attempt + 1);
      }
      const profile = mkdtempSync(join(tmpdir(), "ux-pages-chrome-"));
      const port = 9300 + Math.floor(Math.random() * 500);
      const proc = Bun.spawn(
        [
          "nice", "-n", "19", CHROME, "--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
          "--no-first-run", "--no-default-browser-check", "--disable-gpu", "--hide-scrollbars",
          `--window-size=${opts.width ?? 1440},${opts.height ?? 900}`, "about:blank",
        ],
        { stdout: "ignore", stderr: "ignore" },
      );
      const timer = setTimeout(() => {
        console.error("ux: 3 minutes with the lock; stopping");
        proc.kill(9);
        try {
          rmdirSync(LOCK);
        } catch {}
        process.exit(3);
      }, 180_000);
      const b = new Browser(proc, profile, port, Date.now(), timer);
      for (let i = 0; i < 50; i++) {
        try {
          const r = await fetch(`http://127.0.0.1:${port}/json/version`);
          if (r.ok) return b;
        } catch {}
        await sleep(100);
      }
      await b.close();
      throw new Error("Chrome did not start");
    } catch (e) {
      try {
        rmdirSync(LOCK);
      } catch {}
      throw e;
    }
  }

  async page(): Promise<Page> {
    const t = await (await fetch(`http://127.0.0.1:${this.port}/json/new?about:blank`, { method: "PUT" })).json();
    const p = new Page(t.webSocketDebuggerUrl);
    await p.ready();
    return p;
  }

  async close(): Promise<void> {
    clearTimeout(this.timer);
    this.proc.kill(9);
    await sleep(300);
    // A crashpad handler may linger: it is ours if it names our profile.
    const lingering = Bun.spawnSync(["pgrep", "-f", this.profile]).stdout.toString().split("\n").filter(Boolean);
    for (const pid of lingering) Bun.spawnSync(["kill", "-9", pid]);
    rmSync(this.profile, { recursive: true, force: true });
    try {
      rmdirSync(LOCK);
    } catch {}
  }
}

type Pending = { resolve: (v: any) => void; reject: (e: Error) => void };

export class Page {
  private ws: WebSocket;
  private id = 0;
  private pending = new Map<number, Pending>();
  private waiters: ((m: any) => boolean)[] = [];
  /** console messages and exceptions, for "no console errors". */
  readonly problems: string[] = [];
  private open: Promise<void>;

  constructor(url: string) {
    this.ws = new WebSocket(url);
    this.open = new Promise((res) => (this.ws.onopen = () => res()));
    this.ws.onmessage = (ev) => {
      const m = JSON.parse(String(ev.data));
      if (m.id !== undefined) {
        const p = this.pending.get(m.id);
        this.pending.delete(m.id);
        if (p) m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result);
      } else {
        if (m.method === "Runtime.exceptionThrown") this.problems.push("exception: " + (m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text));
        if (m.method === "Runtime.consoleAPICalled" && ["error", "warning"].includes(m.params.type)) this.problems.push(m.params.type + ": " + m.params.args.map((a: any) => a.value ?? a.description).join(" "));
        if (m.method === "Log.entryAdded" && ["error", "warning"].includes(m.params.entry.level) && !/favicon/.test(m.params.entry.url ?? "")) this.problems.push(m.params.entry.level + ": " + m.params.entry.text + " " + (m.params.entry.url ?? ""));
        this.waiters = this.waiters.filter((w) => !w(m));
      }
    };
  }

  async ready() {
    await this.open;
    for (const d of ["Page", "Runtime", "Log", "DOM"]) await this.send(`${d}.enable`);
    // A layout-shift observer on every document, and a scroll record, before any script of the page runs.
    await this.send("Page.addScriptToEvaluateOnNewDocument", {
      source: `window.__cls = 0; window.__shifts = [];
        try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) { window.__cls += e.value; window.__shifts.push(e.value); } }).observe({ type: 'layout-shift', buffered: true }); } catch (_) {}`,
    });
  }

  send(method: string, params: object = {}): Promise<any> {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  private waitEvent(pred: (m: any) => boolean, ms: number): Promise<boolean> {
    return new Promise((res) => {
      const t = setTimeout(() => res(false), ms);
      this.waiters.push((m) => (pred(m) ? (clearTimeout(t), res(true), true) : false));
    });
  }

  async goto(url: string): Promise<void> {
    const loaded = this.waitEvent((m) => m.method === "Page.loadEventFired", 8000);
    await this.send("Page.navigate", { url });
    await loaded;
    await this.settle();
  }

  /** Wait until the document has been quiet for a moment (fonts, animations, the first paint). */
  async settle(ms = 350) {
    await sleep(ms);
  }

  async eval<T = any>(expr: string): Promise<T> {
    const r = await this.send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(`eval failed: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}\n${expr}`);
    return r.result.value as T;
  }

  /** The bounding box of the first match, in document coordinates (so scrolling does not change it). */
  async box(selector: string): Promise<Box | null> {
    return this.eval(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: Math.round((r.left + scrollX) * 10) / 10, y: Math.round((r.top + scrollY) * 10) / 10, w: Math.round(r.width * 10) / 10, h: Math.round(r.height * 10) / 10 }; })()`);
  }

  async boxes(selectors: string[]): Promise<Record<string, Box | null>> {
    const out: Record<string, Box | null> = {};
    for (const s of selectors) out[s] = await this.box(s);
    return out;
  }

  get scrollY(): Promise<number> {
    return this.eval("Math.round(scrollY)");
  }

  async scrollTo(y: number) {
    await this.eval(`scrollTo(0, ${y})`);
    await sleep(120);
  }

  async cls(): Promise<number> {
    return this.eval("Math.round((window.__cls || 0) * 10000) / 10000");
  }

  /** Click the centre of the first element matching `selector` with a real mouse event; wait for a navigation if one starts. */
  async click(selector: string, opts: { navigates?: boolean } = {}): Promise<boolean> {
    await this.eval(`document.querySelector(${JSON.stringify(selector)})?.scrollIntoView({ block: 'center' })`);
    await sleep(80);
    const c = await this.eval<{ x: number; y: number } | null>(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    if (!c) throw new Error("nothing matches " + selector);
    return this.clickAt(c.x, c.y, opts);
  }

  async clickAt(x: number, y: number, opts: { navigates?: boolean } = {}): Promise<boolean> {
    const nav = this.waitEvent((m) => m.method === "Page.frameNavigated" && !m.params.frame.parentId, opts.navigates === false ? 1 : 2500);
    await this.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await this.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
    await this.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
    const navigated = await nav;
    if (navigated) await this.waitEvent((m) => m.method === "Page.loadEventFired", 5000);
    await this.settle(navigated ? 350 : 250);
    return navigated;
  }

  async key(key: string, code = key) {
    await this.send("Input.dispatchKeyEvent", { type: "keyDown", key, code });
    await this.send("Input.dispatchKeyEvent", { type: "keyUp", key, code });
    await sleep(100);
  }

  async screenshot(path: string, opts: { fullPage?: boolean } = {}) {
    const r = await this.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: !!opts.fullPage });
    await Bun.write(path, Buffer.from(r.data, "base64"));
  }

  url(): Promise<string> {
    return this.eval("location.pathname + location.search + location.hash");
  }

  crumbs(): Promise<string[]> {
    return this.eval(`[...document.querySelectorAll('nav.crumbs a, nav.crumbs .cur')].map((e) => e.textContent.trim())`);
  }
}

/** Compare two sets of boxes; return the sentences for each that moved by more than `tol` pixels. */
export function moved(before: Record<string, Box | null>, after: Record<string, Box | null>, tol = 0.5): string[] {
  const out: string[] = [];
  for (const k of Object.keys(before)) {
    const a = before[k];
    const b = after[k];
    if (!a || !b) {
      if (!!a !== !!b) out.push(`${k}: ${a ? "gone" : "appeared"}`);
      continue;
    }
    const d = (["x", "y", "w", "h"] as const).filter((f) => Math.abs(a[f] - b[f]) > tol).map((f) => `${f} ${a[f]} -> ${b[f]}`);
    if (d.length) out.push(`${k}: ${d.join(", ")}`);
  }
  return out;
}
