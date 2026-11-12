// A page, the console's own script (crates/kafka-console/assets/console.js, unmodified) and a scripted outside: a DOM
// (happy-dom), a fetch whose answers the test decides, an EventSource it can open and break, and timers it runs by hand.
// Nothing here is the script's logic; it only plays the browser's part so that the script's behaviour can be asserted.

import { Window } from "happy-dom";

// CONSOLE_JS names another file to run (the script before a change, to see a test fail on it).
const ASSETS = new URL("../../crates/kafka-console/assets/", import.meta.url).pathname;
const read = (name: string) => Bun.file(ASSETS + name).text();
// CONSOLE_JS names another console.js to run (the script before a change, to see a test fail on it).
const CONSOLE = process.env.CONSOLE_JS ? await Bun.file(process.env.CONSOLE_JS).text() : await read("console.js");
const SCRIPTS = {
  htmx: await read("htmx-2.0.11.min.js"),
  idiomorph: await read("idiomorph-ext-0.8.0.min.js"),
  console: CONSOLE,
  components: await read("components.js"),
  live: await read("live.js"),
  anchor: await read("alpine-anchor-3.17.5.min.js"),
  alpine: await read("alpine-csp-3.17.5.min.js"),
};
// htmx's configuration is the document's own (src/ui/client.rs HTMX_CONFIG): the tests read it from there.
const HTMX_CONFIG = /HTMX_CONFIG: &str = r#"(.*?)"#;/.exec(await Bun.file(new URL("../../crates/kafka-console/src/ui/client.rs", import.meta.url).pathname).text())![1];

export type Reply = { status?: number; etag?: string; body?: string; headers?: Record<string, string>; redirected?: boolean; url?: string };

/** What the page asked of the history: a stand-in that keeps entries and fires popstate on back and forward, as a browser does. */
export class FakeHistory {
  entries: Array<{ state: unknown; url: string }>;
  index = 0;
  pushes: string[] = [];
  replaces: string[] = [];
  constructor(private window: Window, url: string) {
    this.entries = [{ state: null, url }];
  }
  get state(): unknown {
    return this.entries[this.index].state;
  }
  get url(): string {
    return this.entries[this.index].url;
  }
  pushState(state: unknown, _t: string, url?: string): void {
    this.entries = [...this.entries.slice(0, this.index + 1), { state, url: new URL(url ?? this.url, this.url).href }];
    this.index++;
    this.pushes.push(this.url);
  }
  replaceState(state: unknown, _t: string, url?: string): void {
    this.entries[this.index] = { state, url: new URL(url ?? this.url, this.url).href };
    this.replaces.push(this.url);
  }
  back(): void {
    if (this.index === 0) return;
    this.index--;
    this.window.dispatchEvent(new this.window.Event("popstate"));
  }
  forward(): void {
    if (this.index >= this.entries.length - 1) return;
    this.index++;
    this.window.dispatchEvent(new this.window.Event("popstate"));
  }
}

/** A request the script made. */
export type Call = { url: string; method: string; headers: Record<string, string>; signal: AbortSignal | undefined; body: unknown };

/** A fetch the test has not answered yet. */
export type Pending = Call & { answer: (r: Reply) => void; fail: (e: unknown) => void };

export class FakeEventSource {
  static all: FakeEventSource[] = [];
  closed = false;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private listeners = new Map<string, Array<(e: { data: string; lastEventId: string }) => void>>();
  constructor(public url: string) {
    FakeEventSource.all.push(this);
  }
  addEventListener(type: string, fn: (e: { data: string; lastEventId: string }) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }
  close(): void {
    this.closed = true;
  }
  /** The server sent an event. */
  emit(type: string, data: string, lastEventId = ""): void {
    for (const fn of this.listeners.get(type) ?? []) fn({ data, lastEventId });
  }
  open(): void {
    this.onopen?.();
  }
  error(): void {
    this.onerror?.();
  }
}

export type Timer = { fn: () => void; ms: number; cleared: boolean; id?: unknown };

export interface Page {
  history: FakeHistory;
  /** The addresses the script sent the browser to (`location.href = ...`), and how many times it reloaded. */
  navigated: string[];
  reloads: number;
  /** The fades (Element.animate) the script asked for, by the element that was animated. */
  animated: Element[];
  window: Window;
  document: Window["document"];
  /** What the page asked the window to scroll to, in order. */
  scrolls: Array<[number, number]>;
  /** Every fetch and every XMLHttpRequest (htmx's), in order, answered or not. */
  calls: Call[];
  /** The fetches not yet answered. */
  pending: Pending[];
  /** The streams opened, newest last. */
  streams: FakeEventSource[];
  timers: Timer[];
  copied: string[];
  /** The text `document.execCommand('copy')` was asked to copy (the selection of the field the script made). */
  execCopied: string[];
  forms: string[];
  /** Answers every fetch at once with `reply` (a function of the call). */
  autoAnswer(reply: (c: Call) => Reply | Error): void;
  /** Runs the timers that are set (once each), returning how many. */
  runTimers(): number;
  click(el: Element): void;
  flush(): Promise<void>;
}

export interface Options {
  /** Is `navigator.clipboard` there? (Not over plain http on a LAN address.) */
  clipboard?: boolean;
  /** No EventSource at all. */
  noEventSource?: boolean;
  /** The person asked for reduced motion. */
  reduced?: boolean;
  /** The address the page is at (default the console's root). */
  url?: string;
  /** No boost attributes on the body (a page that is not the shell). */
  plain?: boolean;
  /** Start Alpine (the components). Off by default: the tests of console.js do not need it. */
  alpine?: boolean;
}

/** Boots `html` (a body) with the script. */
export async function boot(html: string, opts: Options = {}): Promise<Page> {
  const window = new Window({ url: opts.url ?? "http://127.0.0.1:9093/", width: 1024, height: 768 });
  const document = window.document;
  document.head.innerHTML = `<meta name="htmx-config" content='${HTMX_CONFIG}'><title></title>`;
  document.body.innerHTML = html;
  // The overlay root and the body's boost attributes are the shell's (src/ui/shell.rs, client::body).
  if (!document.getElementById("ovl")) document.body.insertAdjacentHTML("beforeend", '<div id="ovl"></div>');
  if (!opts.plain) {
    document.body.setAttribute("hx-boost", "true");
    document.body.setAttribute("hx-target", "#main");
    document.body.setAttribute("hx-swap", "outerHTML transition:true show:window:top");
    document.body.setAttribute("hx-ext", "morph");
  }
  const calls: Call[] = [];
  const pending: Pending[] = [];
  const timers: Timer[] = [];
  const copied: string[] = [];
  const execCopied: string[] = [];
  const forms: string[] = [];
  let auto: ((c: Call) => Reply | Error) | null = null;

  const response = (r: Reply, signal: AbortSignal | undefined, call_url: string) => {
    const status = r.status ?? 200;
    const aborted = () => new window.DOMException("The operation was aborted.", "AbortError");
    return {
      status,
      ok: status >= 200 && status < 300,
      redirected: r.redirected ?? false,
      url: r.url ?? String(call_url),
      headers: {
        get: (n: string) => {
          const k = n.toLowerCase();
          if (k === "etag") return r.etag ?? null;
          const h = Object.entries(r.headers ?? {}).find(([name]) => name.toLowerCase() === k);
          return h ? h[1] : null;
        },
      },
      // Like a browser: reading the body of an aborted request fails.
      text: async () => {
        if (signal?.aborted) throw aborted();
        return r.body ?? "";
      },
    };
  };

  (window as unknown as { fetch: unknown }).fetch = (url: string, init: { signal?: AbortSignal; headers?: Record<string, string>; method?: string; body?: unknown } = {}) => {
    const call: Call = { url: String(url), method: init.method ?? "GET", headers: { ...(init.headers ?? {}) }, signal: init.signal, body: init.body };
    calls.push(call);
    return new Promise((resolve, reject) => {
      const abort = () => {
        const at = pending.findIndex((q) => q.signal === call.signal && q.url === call.url);
        if (at >= 0) pending.splice(at, 1);
        reject(new window.DOMException("The operation was aborted.", "AbortError"));
      };
      if (call.signal?.aborted) return abort();
      call.signal?.addEventListener("abort", abort);
      const p: Pending = {
        ...call,
        answer: (r) => {
          const at = pending.indexOf(p);
          if (at >= 0) pending.splice(at, 1);
          resolve(response(r, call.signal, call.url));
        },
        fail: (e) => {
          const at = pending.indexOf(p);
          if (at >= 0) pending.splice(at, 1);
          reject(e);
        },
      };
      if (auto) {
        const r = auto(call);
        if (r instanceof Error) p.fail(r);
        else p.answer(r);
      } else pending.push(p);
    });
  };

  // htmx's requests are XMLHttpRequests: the same calls, the same answers as a fetch.
  class FakeXHR {
    readyState = 0;
    status = 0;
    statusText = "";
    responseText = "";
    response = "";
    responseURL = "";
    responseType = "";
    timeout = 0;
    upload = { addEventListener(): void {} };
    addEventListener(): void {}
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    onabort: (() => void) | null = null;
    ontimeout: (() => void) | null = null;
    private method = "GET";
    private url = "";
    private headers: Record<string, string> = {};
    private replyHeaders: Record<string, string> = {};
    open(method: string, url: string): void {
      this.method = method;
      this.url = new URL(url, history.url).href;
    }
    setRequestHeader(k: string, v: string): void {
      this.headers[k] = v;
    }
    overrideMimeType(): void {}
    getResponseHeader(n: string): string | null {
      const h = Object.entries(this.replyHeaders).find(([k]) => k.toLowerCase() === n.toLowerCase());
      return h ? h[1] : null;
    }
    getAllResponseHeaders(): string {
      return Object.entries(this.replyHeaders).map(([k, v]) => `${k}: ${v}`).join("\r\n");
    }
    abort(): void {
      const at = pending.findIndex((q) => (q as unknown as { xhr?: FakeXHR }).xhr === this);
      if (at >= 0) pending.splice(at, 1);
      this.onabort?.();
    }
    send(body?: unknown): void {
      const call: Call = { url: this.url, method: this.method, headers: { ...this.headers }, signal: undefined, body };
      calls.push(call);
      const p = {
        ...call,
        xhr: this,
        answer: (r: Reply) => {
          const at = pending.indexOf(p as unknown as Pending);
          if (at >= 0) pending.splice(at, 1);
          this.status = r.status ?? 200;
          this.responseText = this.response = r.body ?? "";
          this.responseURL = r.url ?? this.url;
          this.replyHeaders = { ...(r.headers ?? {}), ...(r.etag ? { ETag: r.etag } : {}) };
          this.readyState = 4;
          this.onload?.();
        },
        fail: () => {
          const at = pending.indexOf(p as unknown as Pending);
          if (at >= 0) pending.splice(at, 1);
          this.onerror?.();
        },
      } as Pending & { xhr: FakeXHR };
      if (auto) {
        const r = auto(call);
        if (r instanceof Error) p.fail(r);
        else p.answer(r);
      } else pending.push(p);
    }
  }
  (window as unknown as { XMLHttpRequest: unknown }).XMLHttpRequest = FakeXHR;
  // happy-dom's queueMicrotask is a setTimeout, and the timers here are run by hand: Alpine's flush must be a real microtask.
  (window as unknown as { queueMicrotask: unknown }).queueMicrotask = (fn: () => void) => queueMicrotask(fn);
  // Floating UI (inside Alpine's anchor plugin) watches its elements with observers that happy-dom has no layout for.
  for (const name of ["ResizeObserver", "IntersectionObserver"]) {
    (window as unknown as Record<string, unknown>)[name] = class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    };
  }
  // htmx asks `root instanceof Document`; in happy-dom's scope the window's own document is not one. Say that it is.
  const Doc = window.Document as unknown as { [Symbol.hasInstance]?: unknown };
  Object.defineProperty(Doc, Symbol.hasInstance, { value: (o: unknown) => o === window.document, configurable: true });
  // htmx asks an XPathEvaluator for elements with hx-on:* attributes; the console uses none.
  (window as unknown as { XPathEvaluator: unknown }).XPathEvaluator = class {
    createExpression() {
      return { evaluate: () => ({ iterateNext: () => null }) };
    }
  };

  FakeEventSource.all = [];
  if (!opts.noEventSource) (window as unknown as { EventSource: unknown }).EventSource = FakeEventSource;
  // Timers are run by hand (`runTimers`), except when Alpine is on: it flushes its own queue with setTimeout, so they run for real
  // (and a test that waits for a delay waits that long, or reads `timers` for what was asked).
  const realSet = window.setTimeout.bind(window), realClear = window.clearTimeout.bind(window);
  (window as unknown as { setTimeout: unknown }).setTimeout = (fn: () => void, ms: number) => {
    const t: Timer = { fn, ms, cleared: false };
    timers.push(t);
    if (opts.alpine) { t.id = realSet(() => { if (!t.cleared) { t.cleared = true; fn(); } }, ms); return t.id as number; }
    return timers.length;
  };
  (window as unknown as { clearTimeout: unknown }).clearTimeout = (id: number) => {
    if (opts.alpine) { realClear(id); const t = timers.find((x) => x.id === id); if (t) t.cleared = true; return; }
    const t = timers[id - 1];
    if (t) t.cleared = true;
  };
  if (opts.clipboard !== false) {
    Object.defineProperty(window.navigator, "clipboard", { value: { writeText: async (t: string) => void copied.push(t) }, configurable: true });
  } else {
    Object.defineProperty(window.navigator, "clipboard", { value: undefined, configurable: true });
  }
  // `select()` of a field selects all its text (happy-dom only moves the caret): remember which field was selected.
  let selected: { value: string } | null = null;
  const protoSelect = (window.HTMLTextAreaElement.prototype as unknown as { select: () => void });
  protoSelect.select = function (this: { value: string }) {
    selected = this;
  };
  (document as unknown as { execCommand: unknown }).execCommand = (cmd: string) => {
    if (cmd !== "copy") return false;
    execCopied.push(selected ? String(selected.value) : "");
    return true;
  };
  // A form that the script could not post in place is submitted as the page would.
  (window.HTMLFormElement.prototype as unknown as { submit: () => void }).submit = function (this: { action: string }) {
    forms.push(this.action);
  };

  const history = new FakeHistory(window, window.location.href);
  Object.defineProperty(window, "history", { value: history, configurable: true });
  const navigated: string[] = [];
  let reloads = 0;
  Object.defineProperty(window.location, "href", { get: () => history.url, set: (v: string) => void navigated.push(String(v)), configurable: true });
  Object.defineProperty(window.location, "pathname", { get: () => new URL(history.url).pathname, configurable: true });
  Object.defineProperty(window.location, "search", { get: () => new URL(history.url).search, configurable: true });
  Object.defineProperty(window.location, "reload", { value: () => void reloads++, configurable: true });
  (window as unknown as { matchMedia: unknown }).matchMedia = (q: string) => ({ matches: !!opts.reduced && q.includes("reduce"), media: q });
  const scrolls: Array<[number, number]> = [];
  (window as unknown as { scrollTo: unknown }).scrollTo = (x: number, y: number) => void scrolls.push([x, y]);
  const animated: Element[] = [];
  (window.Element.prototype as unknown as { animate: unknown }).animate = function (this: Element) {
    animated.push(this);
    return {};
  };

  window.eval(SCRIPTS.htmx);
  // A browser reads the <meta name="htmx-config"> when the document is ready; happy-dom never says so: apply it the same way.
  const htmx = (window as unknown as { htmx: { config: object; process: (e: Element) => void } }).htmx;
  Object.assign(htmx.config, JSON.parse(HTMX_CONFIG));
  htmx.process(document.body as unknown as Element);
  window.eval(SCRIPTS.idiomorph);
  window.eval(SCRIPTS.console);
  window.eval(SCRIPTS.components);
  window.eval(SCRIPTS.live);
  if (opts.alpine) {
    window.eval(SCRIPTS.anchor);
    window.eval(SCRIPTS.alpine);
  }

  const flush = async () => {
    for (let i = 0; i < 6; i++) await new Promise((r) => setImmediate(r));
  };
  return {
    history,
    navigated,
    get reloads() {
      return reloads;
    },
    animated,
    scrolls,
    window,
    document,
    calls,
    pending,
    streams: FakeEventSource.all,
    timers,
    copied,
    execCopied,
    forms,
    autoAnswer: (reply) => {
      auto = reply;
    },
    runTimers: () => {
      const due = timers.filter((t) => !t.cleared);
      for (const t of due) t.cleared = true;
      for (const t of due) t.fn();
      return due.length;
    },
    click: (el) => {
      el.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
    },
    flush,
  };
}
