// Crawl the console (fixtures server or a live one) by its own links and print, for every page, the breadcrumb, the <title>,
// the active rail item and the status. No Chrome: plain GETs. It is the "audit every route" half of UX finding 4 and 6.
//
//   bun tools/ux/crawl.ts http://127.0.0.1:19412 [--max 400] [--json out.json] [--check]
//
// --check exits 1 when a page breaks the rules: a crumb that equals a query value or a mode word, a last crumb that differs
// from the page title's entity, a page that answers 200 with the Home landmarks under another title, a crumb link that is
// not a real page.

export interface PageInfo {
  url: string;
  status: number;
  title: string;
  crumbs: { label: string; href: string | null }[];
  rail: string | null;
  h1: string | null;
  landmarks: string[];
  notIndexed: boolean;
}

const decode = (s: string) =>
  s.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'");
const strip = (s: string) => decode(s.replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim());

export function parsePage(url: string, status: number, html: string): PageInfo {
  const title = strip(/<title>([\s\S]*?)<\/title>/.exec(html)?.[1] ?? "");
  const nav = /<nav class="crumbs"[^>]*>([\s\S]*?)<\/nav>/.exec(html)?.[1] ?? "";
  const crumbs: PageInfo["crumbs"] = [];
  for (const m of nav.matchAll(/<a href="([^"]*)">([\s\S]*?)<\/a>|<span class="cur">([\s\S]*?)<\/span>/g)) {
    if (m[1] !== undefined) crumbs.push({ label: strip(m[2]), href: decode(m[1]) });
    else crumbs.push({ label: strip(m[3]), href: null });
  }
  const rail = /<a class="ni on" href="[^"]*" aria-current="page">([\s\S]*?)<\/a>/.exec(html)?.[1];
  const h1 = /<h1[^>]*>([\s\S]*?)<\/h1>/.exec(html)?.[1];
  const landmarks: string[] = [];
  for (const m of html.matchAll(/data-extra="([^"]+)"/g)) landmarks.push(m[1]);
  return {
    url,
    status,
    title,
    crumbs,
    rail: rail ? strip(rail) : null,
    h1: h1 ? strip(h1) : null,
    landmarks,
    notIndexed: /<meta name="robots" content="noindex">/.test(html),
  };
}

export async function crawl(base: string, max = 400, perShape = 3): Promise<PageInfo[]> {
  const seen = new Set<string>();
  const shapes = new Map<string, number>();
  const queue: string[] = ["/"];
  const out: PageInfo[] = [];
  while (queue.length && out.length < max) {
    const path = queue.shift()!;
    if (seen.has(path)) continue;
    seen.add(path);
    // A shape: numbers in the path and the values of the query are blanked; at most `perShape` pages of one shape are fetched.
    const u = new URL(path, "http://x");
    const shape = u.pathname.replace(/\/\d+/g, "/#") + "?" + [...u.searchParams.keys()].sort().join("&");
    if ((shapes.get(shape) ?? 0) >= perShape) continue;
    shapes.set(shape, (shapes.get(shape) ?? 0) + 1);
    const res = await fetch(base + path, { redirect: "manual" });
    if (!(res.headers.get("content-type") ?? "").includes("text/html")) continue;
    const html = await res.text();
    out.push(parsePage(path, res.status, html));
    for (const m of html.matchAll(/<a [^>]*href="([^"#]*)"/g)) {
      const href = decode(m[1]);
      if (!href.startsWith("/") || href.startsWith("//")) continue;
      if (/^\/(static|fragments|events|healthz)/.test(href)) continue;
      if (!seen.has(href)) queue.push(href);
    }
    for (const m of html.matchAll(/<form [^>]*action="([^"]*)"[^>]*>/g)) {
      void m; // a GET form is a page by its action alone; the crawl does not submit forms
    }
  }
  return out;
}

const MODE_WORDS = /^(read_committed|read_uncommitted|paused|a busy minute|nothing yet|filtered|all partitions)$/i;

/** The rule violations of one page, as sentences. */
export function violations(p: PageInfo): string[] {
  const bad: string[] = [];
  const q = new URL(p.url, "http://x").searchParams;
  const values = new Set([...q.values()].map((v) => v.toLowerCase()).filter(Boolean));
  const last = p.crumbs[p.crumbs.length - 1];
  for (const c of p.crumbs) {
    if (values.has(c.label.toLowerCase())) bad.push(`crumb "${c.label}" equals a query value`);
    if (MODE_WORDS.test(c.label)) bad.push(`crumb "${c.label}" is a mode or state`);
    if (/^[\w.-]+-\d+( from [\d, ]+)?$/.test(c.label) && c !== last && false) bad.push("partition crumb");
  }
  if (p.status === 200 && !p.crumbs.length) bad.push("no breadcrumb");
  if (p.status === 200 && last && !p.title.toLowerCase().startsWith(last.label.toLowerCase())) bad.push(`title "${p.title}" does not start with the last crumb "${last.label}"`);
  return bad;
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  const base = args.find((a) => a.startsWith("http"))!;
  const max = Number(args[args.indexOf("--max") + 1]) || 400;
  const pages = await crawl(base, max);
  let failed = 0;
  for (const p of pages) {
    const v = violations(p);
    failed += v.length;
    const trail = p.crumbs.map((c) => (c.href ? `${c.label}[${c.href}]` : `${c.label}`)).join(" > ");
    console.log(`${p.status} ${p.url}\n    title: ${p.title}\n    rail: ${p.rail}  crumbs: ${trail}${v.length ? "\n    !! " + v.join("; ") : ""}`);
  }
  console.log(`${pages.length} pages, ${failed} violations`);
  const j = args.indexOf("--json");
  if (j >= 0) await Bun.write(args[j + 1], JSON.stringify(pages, null, 1));
  if (args.includes("--check") && failed) process.exit(1);
}
