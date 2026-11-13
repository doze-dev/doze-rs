// Screenshots of slice R1b's pages and overlays in Firefox and WebKit at 1440 and 1024, for a person to look at.
//   bun --bun shots-r1b.ts <console url> <out dir> <topic> <compacted topic>
// The topics must exist; the script opens overlays and fills forms but never sends a write.
import { firefox, webkit } from "@playwright/test";
const [url = "http://127.0.0.1:29094", out = "shots", topic = "orders", compacted = "inventory.levels"] = process.argv.slice(2);
const shots: [string, string, ((p: import("@playwright/test").Page) => Promise<void>)?][] = [
  ["/topics/new", "create-name", async (p) => {
    await p.getByTestId("name").fill("orders.retry");
    await p.getByTestId("live-check").getByText("Kafka would take this").waitFor();
  }],
  ["/topics/new", "create-taken", async (p) => {
    await p.getByTestId("name").fill(topic);
    await p.getByTestId("field-error").first().waitFor();
  }],
  [`/topics/${topic}?dialog=produce`, "produce", async (p) => {
    await p.getByTestId("key").fill("ord-88420");
    await p.getByTestId("value").fill('{"order_id":"ord-88420","customer":"c-3092","total":7.40}');
    await p.getByRole("textbox", { name: "Header name 1" }).fill("content-type");
    await p.getByRole("textbox", { name: "Header value 1" }).fill("application/json");
    await p.waitForTimeout(600);
  }],
  [`/topics/${topic}?dialog=add-partitions`, "add-partitions"],
  [`/topics/${topic}?dialog=delete-records&partition=0`, "delete-records", async (p) => {
    await p.getByTestId("offset").fill("2");
    await p.waitForTimeout(600);
  }],
  [`/topics/${topic}?dialog=delete`, "delete"],
  [`/topics/${compacted}/config`, "config", async (p) => {
    await p.locator("[data-testid=config-row]").first().getByRole("button", { name: "Where from" }).click();
  }],
  [`/topics/${compacted}/config?edit=min.cleanable.dirty.ratio`, "config-edit", async (p) => {
    await p.getByRole("dialog").getByTestId("config-value").fill("0.7");
    await p.waitForTimeout(600);
  }],
  [`/topics/${compacted}/config?edit=cleanup.policy`, "config-edit-list"],
  [`/topics/${compacted}/config?edit=min.compaction.lag.ms`, "config-edit-lag"],
];
for (const [name, type] of [["firefox", firefox], ["webkit", webkit]] as const) {
  const b = await type.launch();
  for (const width of [1440, 1024]) {
    const p = await b.newPage({ viewport: { width, height: 900 } });
    for (const [path, file, act] of shots) {
      await p.goto(url + path);
      await p.locator("h1").waitFor();
      await p.waitForLoadState("networkidle");
      if (act) await act(p);
      await p.waitForTimeout(400);
      await p.screenshot({ path: `${out}/${file}.${name}.${width}.png`, fullPage: !path.includes("dialog") && !path.includes("edit=") });
    }
    await p.close();
  }
  await b.close();
}
console.log("shots in", out);
