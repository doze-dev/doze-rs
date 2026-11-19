// A console's fetch client. Every request carries the app's proof header (`X-Console: 1` unless it names another): the proof
// the server's API guard asks for, because a page on another site cannot send a custom header without a CORS preflight,
// which a console never grants. A refusal comes back as `{ code, message }` and is thrown as an ApiFailure with those words.
//
// An app makes one client (`createApi`) and builds its queries on it; the event stream of the same client carries the same
// header (the browser's EventSource cannot send one, so the stream is read from a fetch body).
import { untilStaying } from "./leaving";
import { SseParser, type SseEvent } from "./sse";

/** What a console's server says when it refuses. */
export type ApiError = { code: string; message: string };

export class ApiFailure extends Error {
  readonly code: string;
  /** The HTTP status; 0 when the server did not answer at all. */
  readonly status: number;
  /** The state a write's refusal carries beside its words (`{ code, message, detail }`), if any. */
  readonly detail: unknown;
  constructor(status: number, body: ApiError & { detail?: unknown }) {
    super(body.message);
    this.name = "ApiFailure";
    this.code = body.code;
    this.status = status;
    this.detail = body.detail;
  }
}

export function isApiError(v: unknown): v is ApiError {
  return typeof v === "object" && v !== null && typeof (v as ApiError).code === "string" && typeof (v as ApiError).message === "string";
}

/** The console's root when it is served under a prefix, as the server names it in index.html (`<meta name="doze-base">`);
 * empty at `/`. */
export function basePathFromMeta(name = "doze-base"): string {
  const meta = typeof document === "undefined" ? null : document.querySelector(`meta[name="${name}"]`);
  const v = meta?.getAttribute("content") ?? "";
  return v === "/" ? "" : v.replace(/\/$/, "");
}

export type ApiOptions = {
  /** Put before every path. Default: `basePathFromMeta()`. */
  basePath?: string;
  /** The same-origin proof: a header name and its value. Default `["X-Console", "1"]`. */
  proof?: readonly [name: string, value: string];
  /** The words of a request that got no answer (the product is named in them). */
  unreachable?: string;
  /** Sees every refusal body before it is thrown (an app-wide state, like a console that closed its door). */
  onRefusal?: (status: number, body: unknown) => void;
  /** For tests. */
  fetch?: typeof fetch;
};

export type Api = {
  basePath: string;
  /** A JSON request; resolves with the body, rejects with an ApiFailure (or the AbortError of `signal`). */
  api<T>(path: string, init?: RequestInit, signal?: AbortSignal): Promise<T>;
  /** Opens `path` as an event stream and calls `on` for each event until the stream ends or `signal` aborts. Resolves when
   * the stream ends; rejects with an ApiFailure carrying the server's refusal words. The caller reconnects, with
   * `lastEventId`, on its own schedule. */
  streamEvents(path: string, on: (e: SseEvent) => void, signal: AbortSignal, lastEventId?: string): Promise<void>;
};

export function createApi(options: ApiOptions = {}): Api {
  const basePath = options.basePath ?? basePathFromMeta();
  const [proofName, proofValue] = options.proof ?? ["X-Console", "1"];
  const unreachable = options.unreachable ?? "The console did not answer. Is it still running?";
  const send = (url: string, init: RequestInit) => (options.fetch ?? fetch)(url, init);

  const refuse = (status: number, body: unknown): ApiFailure => {
    options.onRefusal?.(status, body);
    return new ApiFailure(status, isApiError(body) ? body : { code: `HTTP_${status}`, message: `The console answered ${status}.` });
  };
  const parse = (text: string): unknown => {
    try {
      return text ? JSON.parse(text) : null;
    } catch {
      return null;
    }
  };

  return {
    basePath,
    async api<T>(path: string, init: RequestInit = {}, signal?: AbortSignal): Promise<T> {
      const headers = new Headers(init.headers);
      headers.set(proofName, proofValue);
      headers.set("Accept", "application/json");
      let res: Response;
      let text: string;
      try {
        // A page being left sends nothing (lib/leaving.ts).
        await untilStaying(signal);
        res = await send(`${basePath}${path}`, { ...init, headers, credentials: "same-origin", signal });
        // The body can be cut too (the server killed mid-answer): the same words, not the browser's "Load failed".
        text = await res.text();
      } catch (e) {
        if ((e as Error).name === "AbortError") throw e;
        throw new ApiFailure(0, { code: "UNREACHABLE", message: unreachable });
      }
      const body = parse(text);
      if (!res.ok) throw refuse(res.status, body);
      return body as T;
    },
    async streamEvents(path, on, signal, lastEventId) {
      const headers = new Headers({ [proofName]: proofValue, Accept: "text/event-stream" });
      if (lastEventId) headers.set("Last-Event-ID", lastEventId);
      await untilStaying(signal);
      const res = await send(`${basePath}${path}`, { headers, credentials: "same-origin", signal, cache: "no-store" });
      if (!res.ok || !res.body) throw refuse(res.status, parse(await res.text().catch(() => "")));
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      const parser = new SseParser();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) return;
        for (const e of parser.push(decoder.decode(value, { stream: true }))) on(e);
      }
    },
  };
}
