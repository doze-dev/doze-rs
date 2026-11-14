// Screenshots of the states of the Groups pages that a demo broker rarely shows (a classic group rebalancing, a KIP-848
// group, commits past the end, the refusals), over the fixtures' world of the `boards` example:
//   cargo run -p kafka-console --features examples --example boards 127.0.0.1:29193
//   bun --bun shots-groups-states.ts http://127.0.0.1:29193 <out dir>
import { firefox, webkit } from "@playwright/test";
const [url = "http://127.0.0.1:29193", out = "shots"] = process.argv.slice(2);
const pages: [string, string, string?][] = [
  ["/groups", "fx-groups"],
  ["/groups/fulfilment", "fx-rebalancing"],
  ["/groups/payments-workers", "fx-848"],
  ["/groups/search-indexer", "fx-classic"],
  ["/groups/analytics/lag", "fx-lag-ahead"],
  ["/groups/analytics/lag?expiring=1", "fx-lag-expiring"],
  ["/groups/search-indexer/lag", "fx-lag"],
  ["/groups/fulfilment/story", "fx-story"],
  ["/groups/search-indexer/story", "fx-story-quiet"],
  ["/groups/fulfilment/story?rule=1", "fx-rule", "[role=dialog]"],
  ["/groups/fulfilment/wire", "fx-wire"],
];
for (const [name, type] of [["firefox", firefox], ["webkit", webkit]] as const) {
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
    // The refusals, as a person meets them: Delete group and Reset offsets of a group with a member.
    await p.goto(`${url}/groups/search-indexer`);
    await p.getByTestId("open-delete").click();
    await p.getByTestId("delete-submit").click();
    await p.getByTestId("refusal").waitFor();
    await p.waitForTimeout(300);
    await p.screenshot({ path: `${out}/fx-delete-refused.${name}.${width}.png` });
    await p.goto(`${url}/groups/search-indexer?dialog=reset`);
    await p.getByTestId("reset-preview-button").click();
    await p.getByTestId("reset-submit").click();
    await p.getByTestId("refusal").waitFor();
    await p.getByTestId("refusal").scrollIntoViewIfNeeded();
    await p.waitForTimeout(300);
    await p.screenshot({ path: `${out}/fx-reset-refused.${name}.${width}.png` });
    await p.goto(`${url}/groups/fulfilment/story`);
    await p.getByTestId("open-rebalance").first().click();
    await p.getByTestId("rebalance-sheet").waitFor();
    await p.waitForTimeout(400);
    await p.screenshot({ path: `${out}/fx-rebalance-sheet.${name}.${width}.png` });
    await p.close();
  }
  await b.close();
}
console.log("shots in", out);
