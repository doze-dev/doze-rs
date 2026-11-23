// Screenshots of every space of the console, light and dark, for a person to look at. Against a doze-aws that is running
// (start one, make some calls so Live has rows, then):
//   bun run shots <out dir> <console url> [browser,browser]
// Files: <out>/<browser>/<page>.<mode>.png at 1440 x 900 (the whole page for Home), plus Live with the drawer of its first
// failed call open, Live paused, and the palette.
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";

const [out = "shots", url = "http://127.0.0.1:4567", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const engines = { firefox, webkit, chromium };
const SPACES = [
  { path: "/", name: "home", title: "Home" },
  { path: "/live", name: "live", title: "Live" },
  { path: "/resources", name: "resources", title: "Resources" },
  { path: "/access", name: "access", title: "Access" },
  { path: "/lab", name: "lab", title: "Lab" },
  { path: "/settings", name: "settings", title: "Settings" },
] as const;

async function settle(p: Page, title: string) {
  await p.locator("h1", { hasText: title }).first().waitFor();
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(400);
}

for (const name of only.split(",") as (keyof typeof engines)[]) {
  const b = await engines[name].launch();
  mkdirSync(`${out}/${name}`, { recursive: true });
  for (const mode of ["light", "dark"] as const) {
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    // The person's choice, as the theme switch stores it.
    await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
    const p = await ctx.newPage();
    for (const s of SPACES) {
      await p.goto(url + s.path);
      await settle(p, s.title);
      if (s.name === "live") await p.getByTestId("live-status").filter({ hasText: "Live" }).waitFor();
      await p.screenshot({ path: `${out}/${name}/${s.name}.${mode}.png`, fullPage: s.name === "home" });
    }
    // Live: the drawer of a call that failed (the first one on the tape), else of the newest call.
    await p.goto(`${url}/live`);
    await settle(p, "Live");
    const rows = p.locator('[data-slot="data-table"] tbody tr[data-row]');
    if ((await rows.count()) > 0) {
      const failed = rows.filter({ has: p.locator('[data-outcome]:not([data-outcome="ok"])') }).first();
      await ((await failed.count()) > 0 ? failed : rows.first()).click();
      await p.getByTestId("call-sheet").waitFor();
      await p.waitForTimeout(350);
      await p.screenshot({ path: `${out}/${name}/live-drawer.${mode}.png` });
      await p.keyboard.press("Escape");
      await p.getByTestId("call-sheet").waitFor({ state: "detached" });
    }
    await p.getByTestId("live-pause").click();
    await p.getByTestId("live-resume").waitFor();
    await p.waitForTimeout(200);
    await p.screenshot({ path: `${out}/${name}/live-paused.${mode}.png` });
    // The palette, open.
    await p.goto(`${url}/`);
    await settle(p, "Home");
    await p.getByTestId("palette-trigger").click();
    await p.getByTestId("palette-input").waitFor();
    await p.waitForTimeout(300);
    await p.screenshot({ path: `${out}/${name}/palette.${mode}.png` });
    await ctx.close();
  }
  await b.close();
}
console.log("shots in", out);
