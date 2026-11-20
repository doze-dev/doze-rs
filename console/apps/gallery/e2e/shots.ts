// Screenshots of every gallery page under every look, for a person to look at.
//   bun run build && bun run preview        (in apps/gallery; serves dist on 127.0.0.1:41731)
//   bun run shots <out dir> [url] [browser,browser]
// Files: <out>/<browser>/<page>.<theme>-<mode>.png (whole page, 1440 wide), and for the overlays an open Dialog, Sheet and
// palette each.
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { at, LOOKS, PAGES } from "./looks";

const [out = "shots", url = "http://127.0.0.1:41731", only = "firefox,webkit,chromium"] = process.argv.slice(2);
const engines = { firefox, webkit, chromium };

async function settle(p: Page) {
  await p.locator("h1").first().waitFor();
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(250);
}

for (const name of only.split(",") as (keyof typeof engines)[]) {
  const b = await engines[name].launch();
  mkdirSync(`${out}/${name}`, { recursive: true });
  for (const look of LOOKS) {
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    const p = await ctx.newPage();
    const tag = `${look.theme}-${look.mode}`;
    for (const page of PAGES) {
      await p.goto(url + at(page.path, look));
      await settle(p);
      await p.screenshot({ path: `${out}/${name}/${page.name}.${tag}.png`, fullPage: true });
    }
    // The overlays, open.
    await p.goto(url + at("/overlays", look));
    await settle(p);
    await p.getByTestId("open-dialog").click();
    await p.getByTestId("dialog").waitFor();
    await p.waitForTimeout(250);
    await p.screenshot({ path: `${out}/${name}/overlay-dialog.${tag}.png` });
    await p.keyboard.press("Escape");
    await p.getByTestId("open-sheet-r1").click();
    await p.getByTestId("sheet").waitFor();
    await p.waitForTimeout(250);
    await p.screenshot({ path: `${out}/${name}/overlay-sheet.${tag}.png` });
    await p.keyboard.press("Escape");
    await p.getByTestId("palette-trigger").click();
    await p.getByTestId("palette-input").waitFor();
    await p.waitForTimeout(250);
    await p.screenshot({ path: `${out}/${name}/overlay-palette.${tag}.png` });
    await p.keyboard.press("Escape");
    await p.getByTestId("open-menu").click();
    await p.getByTestId("menu").waitFor();
    await p.waitForTimeout(250);
    await p.screenshot({ path: `${out}/${name}/overlay-menu.${tag}.png` });
    await ctx.close();
  }
  await b.close();
}
console.log("shots in", out);
