// The comparison's machinery: send one request to every target, keep what came back (status, the headers that matter, the
// body as bytes), and compare the answers with ids made symbolic. See README.md.

export type Target = { name: string; url: string };

export type Step = {
  m: string; // method
  p: string; // path; "@k" is the k-th id this target gave in this scenario (counted from 1)
  b?: string; // body, as sent
  ct?: string | null; // Content-Type; default application/vnd.schemaregistry.v1+json when there is a body, null for none
  h?: Record<string, string>; // other request headers
  note?: string;
};

export type Answer = { status: number; ct: string; www: string; allow: string; body: string; ms: number };

export type Exchange = { step: Step; answers: Record<string, Answer> };

/** What a target keeps while a scenario runs: the ids it gave, in the order they first appeared. */
export class Ids {
  order: number[] = [];
  tok(n: number): string {
    let i = this.order.indexOf(n);
    if (i < 0) {
      this.order.push(n);
      i = this.order.length - 1;
    }
    return `@${i + 1}`;
  }
  path(p: string): string {
    return p.replace(/@(\d+)/g, (_, k) => {
      const id = this.order[Number(k) - 1];
      return id === undefined ? `99999${k}` : String(id);
    });
  }
  /** Learns the ids a body names (every "id": n), without rewriting it. */
  learn(body: string) {
    for (const m of body.matchAll(/"id"\s*:\s*(\d+)/g)) this.tok(Number(m[1]));
  }
  /** The body with every id made symbolic: "id": n, the arrays of ids /referencedby and /schemas/ids/n/... answer. */
  normalize(path: string, body: string): string {
    let out = body
      .replace(/("(?:id|maxId)"\s*:\s*)(\d+)/g, (_, k, n) => `${k}"${this.tok(Number(n))}"`)
      .replace(/(,id=)(\d+)(,)/g, (m, a, n, b) => (this.order.includes(Number(n)) ? `${a}${this.tok(Number(n))}${b}` : m))
      .replace(/(Schema )(\d+)( not found)/g, (m, a, n, b) => (this.order.includes(Number(n)) ? `${a}${this.tok(Number(n))}${b}` : m));
    if (/\/referencedby(\?|$)/.test(path) && /^\[[\d,\s]*\]$/.test(out)) {
      out = out.replace(/\d+/g, (n) => `"${this.tok(Number(n))}"`);
    }
    return out;
  }
}

export async function send(t: Target, ids: Ids, s: Step): Promise<Answer> {
  const headers: Record<string, string> = { ...(s.h ?? {}) };
  if (s.b !== undefined && s.ct !== null) headers["Content-Type"] = s.ct ?? "application/vnd.schemaregistry.v1+json";
  const start = performance.now();
  for (let attempt = 0; ; attempt++) {
    try {
      const r = await fetch(t.url + ids.path(s.p), { method: s.m, headers, body: s.b, redirect: "manual" });
      const body = s.m === "HEAD" ? "" : await r.text();
      ids.learn(body);
      return {
        status: r.status,
        ct: r.headers.get("content-type") ?? "",
        www: r.headers.get("www-authenticate") ?? "",
        allow: r.headers.get("allow") ?? "",
        body,
        ms: Math.round(performance.now() - start),
      };
    } catch (e) {
      if (attempt >= 2) return { status: 0, ct: "", www: "", allow: "", body: `fetch failed: ${e}`, ms: 0 };
      await Bun.sleep(200);
    }
  }
}

/** Removes every subject (soft, then permanently; a referenced one waits for its referrers) and resets the global level and mode. */
export async function reset(t: Target) {
  const ids = new Ids();
  await send(t, ids, { m: "PUT", p: "/mode", b: '{"mode":"READWRITE"}' });
  for (let round = 0; round < 8; round++) {
    const r = await send(t, ids, { m: "GET", p: "/subjects?deleted=true" });
    const subjects: string[] = r.status === 200 ? JSON.parse(r.body) : [];
    if (subjects.length === 0) break;
    for (const s of subjects) {
      const p = `/subjects/${encodeURIComponent(s)}`;
      await send(t, ids, { m: "DELETE", p: `/mode/${encodeURIComponent(s)}` });
      await send(t, ids, { m: "DELETE", p });
      await send(t, ids, { m: "DELETE", p: `${p}?permanent=true` });
    }
  }
  await send(t, ids, { m: "DELETE", p: "/config" });
}

/** The parts of two answers compared: status, content type, challenge, Allow, and the body with ids made symbolic. */
export function comparable(a: Answer, path: string, ids: Ids) {
  return { status: a.status, ct: a.ct, www: a.www, allow: a.allow, body: ids.normalize(path, a.body) };
}

export const j = JSON.stringify;
/** A registration body: {"schema": text, "schemaType"?, "references"?}. */
export function reg(schema: string | object, extra: Record<string, unknown> = {}): string {
  return j({ schema: typeof schema === "string" ? schema : j(schema), ...extra });
}
