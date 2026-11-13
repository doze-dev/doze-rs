// The console's fetch client. Every request carries `X-Console: 1`, the proof the server's API guard asks for (a page on
// another site cannot send a custom header without a CORS preflight, which the console never grants). A refusal comes back
// as `{ code, message }` and is thrown as an ApiFailure with those words.
import type { ApiError } from "./types/ApiError";

/** The console's root (`--console-prefix`), named by the server in index.html; empty at `/`. */
export const basePath: string = (() => {
  const meta = typeof document === "undefined" ? null : document.querySelector('meta[name="doze-base"]');
  const v = meta?.getAttribute("content") ?? "";
  return v === "/" ? "" : v.replace(/\/$/, "");
})();

export class ApiFailure extends Error {
  readonly code: string;
  readonly status: number;
  constructor(status: number, body: ApiError) {
    super(body.message);
    this.name = "ApiFailure";
    this.code = body.code;
    this.status = status;
  }
}

function isApiError(v: unknown): v is ApiError {
  return typeof v === "object" && v !== null && typeof (v as ApiError).code === "string" && typeof (v as ApiError).message === "string";
}

export async function api<T>(path: string, init: RequestInit = {}, signal?: AbortSignal): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("X-Console", "1");
  headers.set("Accept", "application/json");
  let res: Response;
  try {
    res = await fetch(`${basePath}${path}`, { ...init, headers, credentials: "same-origin", signal });
  } catch (e) {
    if ((e as Error).name === "AbortError") throw e;
    throw new ApiFailure(0, { code: "UNREACHABLE", message: "The console did not answer. Is doze-kafka still running?" });
  }
  const text = await res.text();
  let body: unknown;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = null;
  }
  if (!res.ok) {
    throw new ApiFailure(res.status, isApiError(body) ? body : { code: `HTTP_${res.status}`, message: `The console answered ${res.status}.` });
  }
  return body as T;
}
