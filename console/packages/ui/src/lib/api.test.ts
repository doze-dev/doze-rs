import { expect, test } from "bun:test";
import { ApiFailure, createApi } from "./api";

type Call = { url: string; init: RequestInit };

function fake(answer: (call: Call) => Response | Promise<Response>) {
  const calls: Call[] = [];
  const f = (async (url: string, init: RequestInit = {}) => {
    const call = { url, init };
    calls.push(call);
    return answer(call);
  }) as unknown as typeof fetch;
  return { calls, fetch: f };
}

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

test("every request carries the proof header, the base path, and same-origin credentials", async () => {
  const net = fake(() => json(200, { ok: true }));
  const { api } = createApi({ basePath: "/console", fetch: net.fetch });
  expect(await api<{ ok: boolean }>("/api/v1/home", { method: "POST", headers: { "Content-Type": "application/json" } })).toEqual({ ok: true });
  const { url, init } = net.calls[0]!;
  expect(url).toBe("/console/api/v1/home");
  const headers = init.headers as Headers;
  expect(headers.get("X-Console")).toBe("1");
  expect(headers.get("Accept")).toBe("application/json");
  expect(headers.get("Content-Type")).toBe("application/json");
  expect(init.credentials).toBe("same-origin");
  expect(init.method).toBe("POST");
});

test("an app may name its own proof", async () => {
  const net = fake(() => json(200, null));
  await createApi({ basePath: "", proof: ["X-Doze-Console", "yes"], fetch: net.fetch }).api("/x");
  expect((net.calls[0]!.init.headers as Headers).get("X-Doze-Console")).toBe("yes");
});

test("a caller cannot take the proof header away", async () => {
  const net = fake(() => json(200, null));
  await createApi({ basePath: "", fetch: net.fetch }).api("/x", { headers: { "X-Console": "0" } });
  expect((net.calls[0]!.init.headers as Headers).get("X-Console")).toBe("1");
});

test("a refusal is thrown with the server's code, words and detail, and the app sees it first", async () => {
  const seen: unknown[] = [];
  const net = fake(() => json(409, { code: "QUEUE_EXISTS", message: "A queue of that name exists.", detail: { name: "orders" } }));
  const { api } = createApi({ basePath: "", fetch: net.fetch, onRefusal: (status, body) => seen.push([status, body]) });
  const e = await api("/x").catch((err: unknown) => err);
  expect(e).toBeInstanceOf(ApiFailure);
  const f = e as ApiFailure;
  expect([f.status, f.code, f.message, f.detail]).toEqual([409, "QUEUE_EXISTS", "A queue of that name exists.", { name: "orders" }]);
  expect(seen).toHaveLength(1);
});

test("a refusal that is not the console's JSON still has words", async () => {
  const net = fake(() => new Response("<html>bad gateway</html>", { status: 502 }));
  const f = (await createApi({ basePath: "", fetch: net.fetch })
    .api("/x")
    .catch((err: unknown) => err)) as ApiFailure;
  expect([f.status, f.code, f.message]).toEqual([502, "HTTP_502", "The console answered 502."]);
});

test("no answer at all is UNREACHABLE, in the app's words", async () => {
  const net = fake(() => {
    throw new TypeError("Load failed");
  });
  const f = (await createApi({ basePath: "", fetch: net.fetch, unreachable: "The console did not answer. Is doze-aws still running?" })
    .api("/x")
    .catch((err: unknown) => err)) as ApiFailure;
  expect([f.status, f.code, f.message]).toEqual([0, "UNREACHABLE", "The console did not answer. Is doze-aws still running?"]);
});

test("an aborted request rejects with the AbortError, not a failure of the console", async () => {
  const stop = new AbortController();
  stop.abort();
  const net = fake(() => json(200, null));
  const e = (await createApi({ basePath: "", fetch: net.fetch })
    .api("/x", {}, stop.signal)
    .catch((err: unknown) => err)) as Error;
  expect(e.name).toBe("AbortError");
  expect(net.calls).toHaveLength(0);
});

test("the event stream carries the proof and Last-Event-ID, and gives the events as they complete", async () => {
  const enc = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      c.enqueue(enc.encode("id: 7\nevent: call\ndata: {\"a\""));
      c.enqueue(enc.encode(":1}\n\n: keep-alive\n\ndata: plain\n\n"));
      c.close();
    },
  });
  const net = fake(() => new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } }));
  const got: unknown[] = [];
  await createApi({ basePath: "/c", fetch: net.fetch }).streamEvents("/api/v1/tail/live", (e) => got.push(e), new AbortController().signal, "6");
  expect(got).toEqual([
    { event: "call", data: '{"a":1}', id: "7" },
    { event: "message", data: "plain", id: undefined },
  ]);
  const headers = net.calls[0]!.init.headers as Headers;
  expect(net.calls[0]!.url).toBe("/c/api/v1/tail/live");
  expect([headers.get("X-Console"), headers.get("Accept"), headers.get("Last-Event-ID")]).toEqual(["1", "text/event-stream", "6"]);
});

test("a refused stream rejects with the server's words", async () => {
  const net = fake(() => json(403, { code: "FORBIDDEN", message: "Not from this origin." }));
  const f = (await createApi({ basePath: "", fetch: net.fetch })
    .streamEvents("/t", () => {}, new AbortController().signal)
    .catch((err: unknown) => err)) as ApiFailure;
  expect([f.status, f.code, f.message]).toEqual([403, "FORBIDDEN", "Not from this origin."]);
});
