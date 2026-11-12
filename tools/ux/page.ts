// A tab, driven like a person: real mouse and key events through Input.dispatch*, numbers read back with Runtime.evaluate.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { Browser } from "./cdp.ts";

export type Rect = { x: number; y: number; width: number; height: number; top: number; left: number; right: number; bottom: number };

const KEYS: Record<string, { code: string; vk: number; text?: string }> = {
  Enter: { code: "Enter", vk: 13, text: "\r" },
  Escape: { code: "Escape", vk: 27 },
  ArrowDown: { code: "ArrowDown", vk: 40 },
  ArrowUp: { code: "ArrowUp", vk: 38 },
  ArrowLeft: { code: "ArrowLeft", vk: 37 },
  ArrowRight: { code: "ArrowRight", vk: 39 },
  Home: { code: "Home", vk: 36 },
  End: { code: "End", vk: 35 },
  Tab: { code: "Tab", vk: 9 },
  " ": { code: "Space", vk: 32, text: " " },
  Backspace: { code: "Backspace", vk: 8 },
};

/** Installed in every document before its scripts: layout shifts, long tasks and the page's own errors. */
const INSTRUMENT = `
  window.__ls = { value: 0, entries: [] };
  try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) { window.__ls.value += e.value; window.__ls.entries.push({ value: e.value, at: Math.round(e.startTime), nodes: (e.sources || []).map((s) => (s.node && (s.node.id || s.node.className || s.node.nodeName)) || '?') }); } }).observe({ type: 'layout-shift', buffered: true }); } catch (e) {}
`;

export class Page {
  /** Console problems seen since the last `clearLogs`: errors, warnings, exceptions, CSP violations. */
  logs: string[] = [];
  /** Requests the page made (url and the htmx headers), newest last. */
  requests: Array<{ url: string; method: string; headers: Record<string, string> }> = [];
  constructor(private b: Browser, private sessionId: string, private base: string) {
    b.on((method, p, sid) => {
      if (sid !== sessionId) return;
      if (method === "Runtime.exceptionThrown") this.logs.push(`exception: ${JSON.stringify((p as { exceptionDetails?: { text?: string; exception?: { description?: string } } }).exceptionDetails?.exception?.description ?? p)}`);
      else if (method === "Runtime.consoleAPICalled") {
        const t = (p as { type: string }).type;
        if (t === "error" || t === "warning") this.logs.push(`console.${t}: ${((p as { args: Array<{ value?: unknown; description?: string }> }).args ?? []).map((a) => a.value ?? a.description).join(" ")}`);
      } else if (method === "Log.entryAdded") {
        const e = (p as { entry: { level: string; text: string; url?: string } }).entry;
        if (e.level === "error" || e.level === "warning") this.logs.push(`log.${e.level}: ${e.text} ${e.url ?? ""}`);
      } else if (method === "Network.requestWillBeSent") {
        const r = (p as { request: { url: string; method: string; headers: Record<string, string> } }).request;
        this.requests.push({ url: r.url, method: r.method, headers: r.headers });
      }
    });
  }

  private cmd(method: string, params: Record<string, unknown> = {}) { return this.b.send(method, params, this.sessionId); }

  async init(): Promise<void> {
    await this.cmd("Page.addScriptToEvaluateOnNewDocument", { source: INSTRUMENT });
    await this.cmd("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  }

  clearLogs(): void { this.logs = []; this.requests = []; }

  async goto(path: string): Promise<void> {
    await this.cmd("Page.navigate", { url: path.startsWith("http") ? path : this.base + path });
    await this.ready();
  }

  async ready(): Promise<void> {
    await this.waitFor(`document.readyState === 'complete' && !!window.htmx && !!window.Alpine`, 8000);
    await Bun.sleep(80);
  }

  /** Runs `expr` in the page and returns its value (promises are awaited). */
  async eval<T = unknown>(expr: string): Promise<T> {
    const r = (await this.cmd("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true })) as { result: { value?: T }; exceptionDetails?: { exception?: { description?: string }; text: string } };
    if (r.exceptionDetails) throw new Error(`eval failed: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}\n  in: ${expr.slice(0, 160)}`);
    return r.result.value as T;
  }

  async waitFor(expr: string, ms = 3000): Promise<void> {
    const t0 = Date.now();
    for (;;) {
      let ok = false;
      try { ok = !!(await this.eval(expr)); } catch { /* the document is changing */ }
      if (ok) return;
      if (Date.now() - t0 > ms) throw new Error(`timed out waiting for: ${expr}`);
      await Bun.sleep(40);
    }
  }

  async rect(sel: string): Promise<Rect> {
    const r = await this.eval<Rect | null>(`(() => { const e = document.querySelector(${JSON.stringify(sel)}); if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height, top: r.top, left: r.left, right: r.right, bottom: r.bottom }; })()`);
    if (!r) throw new Error(`no element for ${sel}`);
    return r;
  }

  async exists(sel: string): Promise<boolean> { return this.eval<boolean>(`!!document.querySelector(${JSON.stringify(sel)})`); }
  async text(sel: string): Promise<string> { return this.eval<string>(`(document.querySelector(${JSON.stringify(sel)}) || {}).textContent || ''`); }
  async scrollY(): Promise<number> { return this.eval<number>("window.scrollY"); }
  async scrollTo(y: number): Promise<void> { await this.eval(`window.scrollTo(0, ${y})`); await Bun.sleep(60); }
  async activeId(): Promise<string> { return this.eval<string>("(document.activeElement && (document.activeElement.id || document.activeElement.getAttribute('data-test') || document.activeElement.tagName)) || ''"); }

  /** Layout-shift total since the last `resetShift` (the sum of `layout-shift` entries without recent input). */
  async shift(): Promise<number> { return this.eval<number>("window.__ls ? window.__ls.value : -1"); }
  async shiftEntries(): Promise<unknown[]> { return this.eval<unknown[]>("window.__ls ? window.__ls.entries : []"); }
  async resetShift(): Promise<void> { await this.eval("window.__ls && (window.__ls.value = 0, window.__ls.entries = [])"); }

  async move(x: number, y: number): Promise<void> {
    await this.cmd("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  }

  /** A real click at the centre of the element (scrolled into view first, like a person would). */
  async click(sel: string, opts: { scroll?: boolean } = {}): Promise<void> {
    if (opts.scroll !== false) await this.eval(`document.querySelector(${JSON.stringify(sel)}).scrollIntoView({ block: 'nearest' })`);
    const r = await this.rect(sel);
    const x = r.x + r.width / 2, y = r.y + r.height / 2;
    await this.move(x, y);
    await this.cmd("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 });
    await this.cmd("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 });
  }

  async hover(sel: string): Promise<void> {
    await this.eval(`document.querySelector(${JSON.stringify(sel)}).scrollIntoView({ block: 'center' })`);
    const r = await this.rect(sel);
    await this.move(r.x + r.width / 2, r.y + r.height / 2);
  }

  async key(name: string, mods = 0): Promise<void> {
    const k = KEYS[name];
    if (k) {
      await this.cmd("Input.dispatchKeyEvent", { type: k.text ? "keyDown" : "rawKeyDown", key: name, code: k.code, windowsVirtualKeyCode: k.vk, text: k.text, modifiers: mods });
      await this.cmd("Input.dispatchKeyEvent", { type: "keyUp", key: name, code: k.code, windowsVirtualKeyCode: k.vk, modifiers: mods });
    } else {
      await this.cmd("Input.dispatchKeyEvent", { type: "keyDown", key: name, text: name, modifiers: mods });
      await this.cmd("Input.dispatchKeyEvent", { type: "keyUp", key: name, modifiers: mods });
    }
  }

  async type(text: string): Promise<void> { for (const ch of text) await this.key(ch); }
  async focus(sel: string): Promise<void> { await this.eval(`document.querySelector(${JSON.stringify(sel)}).focus()`); }

  async back(): Promise<void> { await this.eval("history.back()"); await Bun.sleep(150); }

  async shot(path: string, clip?: { x: number; y: number; width: number; height: number }): Promise<string> {
    const r = (await this.cmd("Page.captureScreenshot", { format: "png", ...(clip ? { clip: { ...clip, scale: 1 } } : {}) })) as { data: string };
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, Buffer.from(r.data, "base64"));
    return path;
  }

  /** Slows the whole tab's animations to `rate` of real time (0 pauses), for a frame-by-frame look. */
  async animationRate(rate: number): Promise<void> { await this.cmd("Animation.enable"); await this.cmd("Animation.setPlaybackRate", { playbackRate: rate }); }
}
