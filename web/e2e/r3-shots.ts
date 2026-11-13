// Screenshots of the Live pages: bun shots.ts <engine> <width> <url-path> <name> [tail]
import { firefox, webkit, chromium } from "@playwright/test";
const [engine, width, path, name, mode] = process.argv.slice(2);
const b = await ({ firefox, webkit, chromium } as const)[engine as "firefox"].launch();
const page = await b.newPage({ viewport: { width: Number(width), height: 1000 } });
const base = process.env.URL ?? "http://127.0.0.1:29394";
await page.goto(base + path);
await page.waitForTimeout(1500);
if (mode === "tail") { await page.waitForTimeout(5000); }
const info = await page.evaluate(() => {
  const s = document.querySelector("[data-testid=tape] [data-slot=data-table]") as HTMLElement | null;
  const cols = [...document.querySelectorAll("[data-testid=tape] thead th")].map((t) => Math.round(t.getBoundingClientRect().width));
  return { doc: document.documentElement.scrollWidth - document.documentElement.clientWidth, table: s ? s.scrollWidth - s.clientWidth : null, tw: s?.clientWidth, cols, main: document.getElementById("main")?.clientWidth };
});
console.log(engine, width, path, JSON.stringify(info));
const out = `/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/r3/shots/${name}-${engine}-${width}.png`;
await page.screenshot({ path: out, fullPage: false });
console.log(out);
await b.close();
