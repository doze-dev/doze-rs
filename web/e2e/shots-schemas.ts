// Screenshots of the Schemas pages and the schema parts of the topic pages in Firefox and WebKit, 1440 and 1024, light and dark,
// for a person to look at.
//   bun --bun shots-schemas.ts <console url> <out dir> <topic> <subject>
// The broker needs a registry with `subject` (with two versions or more) and records in the wire format on `topic`.
import { firefox, webkit, type Page } from "@playwright/test";
const [url = "http://127.0.0.1:29311", out = "shots", topic = "shop.orders", subject = "shop.orders-value"] = process.argv.slice(2);
const s = encodeURIComponent(subject);
const t = encodeURIComponent(topic);
const pages: [string, string, ((p: Page) => Promise<void>)?][] = [
  ["list", "/schemas"],
  ["subject", `/schemas/subjects/${s}`],
  ["subject-stored", `/schemas/subjects/${s}?version=1&stored=1`],
  ["compatibility", `/schemas/subjects/${s}?dialog=compatibility`],
  ["try", `/schemas/subjects/${s}?dialog=try`],
  ["delete-subject", `/schemas/subjects/${s}?dialog=delete-subject`],
  ["diff", `/schemas/subjects/${s}/diff`],
  ["diff-unified", `/schemas/subjects/${s}/diff?layout=unified`],
  ["new", "/schemas/new"],
  [
    "new-incompatible",
    `/schemas/new?subject=${s}`,
    async (p) => {
      const box = p.getByTestId("schema");
      const text = await box.inputValue();
      await box.fill(text.replace(/\n  \]/, ',\n    {"name": "qty", "type": "int"}\n  ]'));
      await p.getByTestId("findings").waitFor();
    },
  ],
  ["messages", `/topics/${t}/messages`],
  [
    "record",
    `/topics/${t}/messages`,
    async (p) => {
      await p.getByTestId("open-record").first().click();
      await p.getByTestId("record-sheet").waitFor();
    },
  ],
  ["topic-schema", `/topics/${t}/schema`],
  ["produce", `/topics/${t}/messages?dialog=produce`],
  ["topics", "/topics"],
  ["connect", "/connect"],
];

async function settle(p: Page) {
  await p.locator("h1").first().waitFor();
  await p.waitForLoadState("networkidle");
  await p.waitForTimeout(500);
}

for (const [name, type] of [["firefox", firefox], ["webkit", webkit]] as const) {
  const b = await type.launch();
  for (const scheme of ["light", "dark"] as const) {
    for (const width of [1440, 1024]) {
      const ctx = await b.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
      await ctx.addInitScript((dark) => localStorage.setItem("doze-kafka.theme", dark ? "dark" : "light"), scheme === "dark");
      const p = await ctx.newPage();
      for (const [label, path, act] of pages) {
        await p.goto(url + path);
        await settle(p);
        if (act) {
          await act(p);
          await p.waitForTimeout(500);
        }
        const over = await p.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
        if (over > 0) console.log(`SIDEWAYS ${over}px: ${label} ${name} ${width} ${scheme}`);
        await p.screenshot({ path: `${out}/${label}.${name}.${width}.${scheme}.png`, fullPage: !act && !path.includes("dialog") });
      }
      await ctx.close();
    }
  }
  await b.close();
}
console.log("shots in", out);
