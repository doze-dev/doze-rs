// A page, the console's own script (crates/kafka-console/assets/console.js, unmodified) and a scripted outside: a DOM
// (happy-dom), a fetch whose answers the test decides, an EventSource it can open and break, and timers it runs by hand.
// Nothing here is the script's logic; it only plays the browser's part so that the script's behaviour can be asserted.

import { Window } from "happy-dom";

// CONSOLE_JS names another file to run (the script before a change, to see a test fail on it).
const SCRIPT = await Bun.file(process.env.CONSOLE_JS ?? new URL("../../crates/kafka-console/assets/console.js", import.meta.url).pathname).text();

export type Reply = { status?: number; etag?: string; body?: string };

/** A request the script made. */
export type Call = { url: string; method: string; headers: Record<string, string>; signal: AbortSignal | undefined; body: unknown };

/** A fetch the test has not answered yet. */
export type Pending = Call & { answer: (r: Reply) => void; fail: (e: unknown) => void };

export class FakeEventSource {
  static all: FakeEventSource[] = [];
  closed = false;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private listeners = new Map<string, Array<(e: { data: string }) => void>>();
  constructor(public url: string) {
    FakeEventSource.all.push(this);
  }
  addEventListener(type: string, fn: (e: { data: string }) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }
  close(): void {
    this.closed = true;
  }
  /** The server sent an event. */
  emit(type: string, data: string): void {
    for (const fn of this.listeners.get(type) ?? []) fn({ data });
  }
  open(): void {
    this.onopen?.();
  }
  error(): void {
    this.onerror?.();
  }
}

export type Timer = { fn: () => void; ms: number; cleared: boolean };

export interface Page {
  window: Window;
  document: Window["document"];
  /** Every fetch, in order, answered or not. */
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
  tick(ids: string): void;
  flush(): Promise<void>;
}

export interface Options {
  /** The stream's URL on the body (what the server renders); none for a page that does not listen. */
  stream?: string | null;
  /** Is `navigator.clipboard` there? (Not over plain http on a LAN address.) */
  clipboard?: boolean;
  /** No EventSource at all. */
  noEventSource?: boolean;
}

/** Boots `html` (a body) with the script. */
export async function boot(html: string, opts: Options = {}): Promise<Page> {
  const window = new Window({ url: "http://127.0.0.1:9093/", width: 1024, height: 768 });
  const document = window.document;
  document.body.innerHTML = html;
  if (opts.stream) document.body.setAttribute("data-stream", opts.stream);
  const calls: Call[] = [];
  const pending: Pending[] = [];
  const timers: Timer[] = [];
  const copied: string[] = [];
  const execCopied: string[] = [];
  const forms: string[] = [];
  let auto: ((c: Call) => Reply | Error) | null = null;

  const response = (r: Reply, signal: AbortSignal | undefined) => {
    const status = r.status ?? 200;
    const aborted = () => new window.DOMException("The operation was aborted.", "AbortError");
    return {
      status,
      ok: status >= 200 && status < 300,
      headers: { get: (n: string) => (n.toLowerCase() === "etag" ? (r.etag ?? null) : null) },
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
          resolve(response(r, call.signal));
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

  FakeEventSource.all = [];
  if (!opts.noEventSource) (window as unknown as { EventSource: unknown }).EventSource = FakeEventSource;
  (window as unknown as { setTimeout: unknown }).setTimeout = (fn: () => void, ms: number) => {
    timers.push({ fn, ms, cleared: false });
    return timers.length;
  };
  (window as unknown as { clearTimeout: unknown }).clearTimeout = (id: number) => {
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

  window.eval(SCRIPT);

  const flush = async () => {
    for (let i = 0; i < 6; i++) await new Promise((r) => setImmediate(r));
  };
  return {
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
    tick: (ids) => {
      const es = FakeEventSource.all[FakeEventSource.all.length - 1];
      es.emit("tick", ids);
    },
    flush,
  };
}
