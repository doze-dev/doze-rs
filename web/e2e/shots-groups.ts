// Screenshots of the Groups pages (R2) in Firefox and WebKit (and Chromium with `all`) at 1440 and 1024, for a person to
// look at. Against a console serving a broker with traffic (the demo workload has fulfilment, analytics, payments-workers).
//   bun --bun shots-groups.ts <console url> <out dir> [all]
import { chromium, firefox, webkit } from "@playwright/test";
const [url = "http://127.0.0.1:29094", out = "shots", which] = process.argv.slice(2);
const browsers = [["firefox", firefox], ["webkit", webkit], ...(which === "all" ? ([["chromium", chromium]] as const) : [])] as const;
const pages: [string, string, string?][] = [
  ["/groups", "groups"],
  ["/groups?state=%5B%22stable%22%5D", "groups-stable"],
  ["/groups/fulfilment", "fulfilment"],
  ["/groups/analytics", "analytics"],
  ["/groups/payments-workers", "payments-workers"],
  ["/groups/fulfilment/lag", "fulfilment-lag"],
  ["/groups/analytics/lag?expiring=1", "analytics-expiring"],
  ["/groups/fulfilment/story", "fulfilment-story"],
  ["/groups/fulfilment/wire", "fulfilment-wire"],
  ["/groups/fulfilment?dialog=reset", "fulfilment-reset", "[role=dialog]"],
  ["/groups/fulfilment?dialog=delete", "fulfilment-delete", "[role=dialog]"],
  ["/groups/analytics/lag?dialog=delete-offsets", "analytics-delete-offsets", "[role=dialog]"],
];
for (const [name, type] of browsers) {
  const b = await type.launch();
  for (const width of [1440, 1024]) {
    const p = await b.newPage({ viewport: { width, height: 900 } });
    for (const [path, file, wait] of pages) {
      await p.goto(url + path);
      await p.locator("h1").waitFor();
      if (wait) await p.locator(wait).waitFor();
      await p.waitForLoadState("networkidle");
      await p.waitForTimeout(500);
      await p.screenshot({ path: `${out}/${file}.${name}.${width}.png`, fullPage: !wait });
    }
    // A member's Sheet and a rebalance's Sheet, opened by a click as a person would.
    await p.goto(`${url}/groups/fulfilment`);
    await p.getByTestId("member-link").first().click();
    await p.getByTestId("member-sheet").waitFor();
    await p.waitForTimeout(400);
    await p.screenshot({ path: `${out}/member-sheet.${name}.${width}.png` });
    await p.goto(`${url}/groups/fulfilment/story`);
    await p.getByTestId("group-story").waitFor();
    const open = p.getByTestId("open-rebalance").first();
    if (await open.count()) {
      await open.click();
      await p.getByTestId("rebalance-sheet").waitFor();
      await p.waitForTimeout(400);
      await p.screenshot({ path: `${out}/rebalance-sheet.${name}.${width}.png` });
    }
    await p.close();
  }
  await b.close();
}
console.log("shots in", out);
