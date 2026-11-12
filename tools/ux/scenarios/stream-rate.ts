// How often the stream tells a page to refetch, per page, over 10 s, against a REAL broker with traffic (LIVE_URL, default
// http://127.0.0.1:9094): the page's own subscription (the ids in its `data-stream`, with each fragment's address, as the script
// sends them), and for comparison the same ids the way the old stream sent them (no addresses: a tick on every bump).
// Targets: a calm page 0 (it watches nothing), Home and a state-like fragment at most 2 per 10 s while the data is unchanged and never
// without a change, Live at most 10 per 10 s.
import type { T } from "../drive.ts";

const LIVE = process.env.LIVE_URL ?? "http://127.0.0.1:9094";

async function ticks(url: string, seconds: number): Promise<number> {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), seconds * 1000);
  let n = 0;
  try {
    const res = await fetch(url, { signal: ctl.signal, headers: { accept: "text/event-stream" } });
    const reader = res.body!.getReader();
    let buf = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += new TextDecoder().decode(value);
      const parts = buf.split("\n\n");
      buf = parts.pop() ?? "";
      for (const p of parts) if (p.includes("event: tick")) n++;
    }
  } catch { /* the time is up */ }
  clearTimeout(timer);
  return n - 1; // the first event is the page's own reconciliation, not an update
}

const PAGES: [string, string][] = [
  ["Home", "/"], ["Live", "/live"], ["Topics list", "/topics"], ["a topic", "/topics/orders"], ["Groups list", "/groups"], ["a group's Lag", "/groups/fulfilment/lag"], ["Config", "/topics/orders/config"],
];

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "stream-rate"(t: T) {
    try { await fetch(LIVE + "/live", { signal: AbortSignal.timeout(2000) }); } catch { t.num("skipped", "no real broker at " + LIVE); return; }
    const jobs = PAGES.map(async ([name, path]) => {
      const html = await (await fetch(LIVE + path)).text();
      const stream = /data-stream="([^"]*)"/.exec(html)?.[1]?.replace(/&amp;/g, "&");
      if (!stream) return { name, now: 0, before: 0, ids: "(no stream)" };
      const ids = new URL(stream, LIVE).searchParams.get("watch") ?? "";
      const frags = [...ids.split(",")].map((id) => new RegExp(`id="${id}"[^>]*data-frag="([^"]*)"|data-frag="([^"]*)"[^>]*id="${id}"`).exec(html)).map((m) => m && (m[1] ?? m[2])).filter(Boolean) as string[];
      const withF = LIVE + stream + frags.map((u) => "&f=" + encodeURIComponent(u.replace(/&amp;/g, "&"))).join("");
      const [now, before] = await Promise.all([ticks(withF, 10), ticks(LIVE + stream, 10)]);
      return { name, now, before, ids };
    });
    for (const r of await Promise.all(jobs)) {
      t.num(`${r.name} (${r.ids}): ticks per 10 s now / the old stream`, `${r.now} / ${r.before}`);
      const limit = r.name === "Live" ? 10 : 2;
      t.ok(r.now <= limit, `${r.name}: ${r.now} ticks in 10 s (at most ${limit})`);
    }
  },
};
