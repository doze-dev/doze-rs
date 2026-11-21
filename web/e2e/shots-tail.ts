// Screenshots of the Live tail page (CON-1580) in light and dark, for a person to look at: the chooser, a running tail of
// several topics with real traffic, the filters, the held pill, a row's headers and menu, the record Sheet, and the entry
// points on a topic and a group. Run against a console serving a broker with traffic (the page reads what is written).
//   bun --bun shots-tail.ts <console url> <out dir> [chromium|firefox|webkit]
import { chromium, firefox, webkit, type Page } from "@playwright/test";
const [url = "http://127.0.0.1:39922", out = "shots", which = "chromium"] = process.argv.slice(2);
const type = { chromium, firefox, webkit }[which as "chromium"] ?? chromium;
const b = await type.launch();
const settle = (p: Page, ms = 600) => p.waitForTimeout(ms);
for (const theme of ["light", "dark"] as const) {
  for (const width of [1440, 1024]) {
    const ctx = await b.newContext({ viewport: { width, height: 900 }, colorScheme: theme });
    const p = await ctx.newPage();
    await p.goto(url + "/tail");
    await p.evaluate((t) => {
      localStorage.setItem("doze-kafka.theme", t);
      localStorage.setItem("doze:tail-speed", "realtime");
    }, theme);
    const shot = async (name: string, full = false) => p.screenshot({ path: `${out}/${name}.${theme}.${width}.${which}.png`, fullPage: full });
    await p.goto(url + "/tail");
    await p.locator("h1").waitFor();
    await settle(p);
    await shot("choose");
    await p.goto(url + "/tail?topic=payments&pattern=orders.*&from=last&last=10");
    await p.getByTestId("tail-conn").and(p.locator("[data-state=live]")).waitFor({ timeout: 15_000 });
    await p.mouse.move(2, 2);
    await settle(p, 4500);
    await shot("running");
    await shot("running-full", true);
    // Held: the pointer on a row while records come.
    await p.getByTestId("tail-wrap").locator("tbody tr[data-row]").nth(4).hover();
    await settle(p, 4000);
    await shot("held");
    if (width === 1440) {
      await p.getByTestId("tail-wrap").locator("tbody tr[data-row]").nth(1).getByTestId("row-headers").click();
      await settle(p, 400);
      await shot("headers");
      await p.keyboard.press("Escape");
      await settle(p, 500);
      await p.getByTestId("tail-wrap").locator("tbody tr[data-row]").nth(2).getByTestId("row-menu").click();
      await settle(p, 400);
      await shot("row-menu");
      await p.keyboard.press("Escape");
      await settle(p, 500);
    }
    await p.mouse.move(2, 2);
    await p.getByTestId("tail-filters").click();
    await settle(p, 400);
    await shot("filters");
    await p.getByLabel("JSON field").fill("state");
    await p.getByLabel("Equals").fill("paid");
    await p.getByTestId("filters-apply").click();
    await p.mouse.move(2, 2);
    await settle(p, 5000);
    await shot("filtered");
    await p.getByTestId("tail-wrap").locator("tbody tr[data-row]").first().click();
    await p.getByTestId("record-sheet").waitFor();
    await settle(p, 800);
    await shot("sheet");
    await p.keyboard.press("Escape");
      await settle(p, 500);
    await p.keyboard.press("p");
    await settle(p, 400);
    await shot("paused");
    if (width === 1440) {
      await p.goto(url + "/topics/orders.eu");
      await p.locator("h1").waitFor();
      await settle(p);
      await shot("topic-entry");
      await p.goto(url + "/groups/fulfilment");
      await p.locator("h1").waitFor();
      await settle(p);
      await shot("group-entry");
    }
    await ctx.close();
  }
}
await b.close();
console.log("shots in", out);
