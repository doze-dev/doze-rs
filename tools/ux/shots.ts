// Screenshots of pages at the two sizes a designer checks (1440x900, 1024x768), for LOOKING at (the Read tool), with the numbers a
// script can check: horizontal overflow, text clipped by its box, controls under 24 px.
//
//   bun tools/ux/shots.ts --url http://127.0.0.1:19311 --out target/ux/shots/after [--sizes 1440x900,1024x768] [--full] /topics /groups ...
//
// Run it under the shared Chrome lock (docs/port/console/ux-tests.md).
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { Browser } from "./cdp.ts";
import { Page } from "./page.ts";

const args = process.argv.slice(2);
const opt = (n: string, d = "") => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : d; };
const base = opt("--url").replace(/\/$/, "");
const out = opt("--out", "target/ux/shots");
const sizes = opt("--sizes", "1440x900,1024x768").split(",").map((s) => s.split("x").map(Number) as [number, number]);
const evalJs = opt("--eval");
const paths = args.filter((a, i) => a.startsWith("/") && args[i - 1] !== "--url" && args[i - 1] !== "--out" && args[i - 1] !== "--eval");
mkdirSync(out, { recursive: true });

const CHECK = `(() => {
  const over = document.documentElement.scrollWidth - document.documentElement.clientWidth;
  const small = [...document.querySelectorAll('main a[href], main button, main input, main select')].filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && r.height < 24 && getComputedStyle(e).visibility !== 'hidden' && !e.closest('[hidden]') && !(e.tagName === 'A' && getComputedStyle(e).display === 'inline'); }).map((e) => (e.textContent || e.getAttribute('aria-label') || e.name || '').trim().slice(0, 24));
  const clipped = [...document.querySelectorAll('main *')].filter((e) => e.children.length === 0 && e.scrollWidth > e.clientWidth + 1 && getComputedStyle(e).overflow !== 'visible' && getComputedStyle(e).textOverflow !== 'ellipsis' && e.clientWidth > 0).map((e) => e.tagName + '.' + e.className).slice(0, 5);
  return { over, small: small.slice(0, 6), clipped };
})()`;

const b = await Browser.launch();
try {
  const tab = await b.tab();
  const p = new Page(b, tab.sessionId, base);
  await p.init();
  for (const [w, h] of sizes) {
    await b.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 1, mobile: false }, tab.sessionId);
    for (const path of paths) {
      await p.goto(path);
      await Bun.sleep(350);
      const name = path.replace(/^\//, "").replace(/[^A-Za-z0-9.]+/g, "_") || "home";
      const file = join(out, `${name}.${w}.png`);
      await p.shot(file);
      const c = await p.eval<{ over: number; small: string[]; clipped: string[] }>(CHECK);
      const flags = [c.over > 0 ? `overflow ${c.over}px` : "", c.small.length ? `small: ${c.small.join(", ")}` : "", c.clipped.length ? `clipped: ${c.clipped.join(", ")}` : ""].filter(Boolean);
      if (evalJs) console.log("   eval:", JSON.stringify(await p.eval(evalJs)));
      console.log(`${w} ${path} -> ${file}${flags.length ? "   !! " + flags.join(" | ") : ""}`);
    }
  }
} finally {
  b.kill();
}
