// Screenshots of the Lab's Rehearse tab with rules in force, the rule dialog, a rule's sheet and the forced-event forms, light and dark, at
// 1440 x 900. Against a doze-aws that is running (it makes its own queue, rules and forced events):
//   bun --bun e2e/lab-rules-shots.ts <out dir> <console url> <endpoint> [browser]
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";
import { aws } from "./server";

const [out = "shots", url = "http://127.0.0.1:4567", endpoint = "http://127.0.0.1:4566", only = "chromium"] = process.argv.slice(2);
const engines = { firefox, webkit, chromium };
const headers = { "X-Console": "1", Origin: url, "Content-Type": "application/json" };
const post = async (path: string, body: unknown) => (await fetch(`${url}${path}`, { method: "POST", headers, body: JSON.stringify(body) })).json();

async function seed() {
  const made = JSON.parse(aws(endpoint, ["sqs", "create-queue", "--queue-name", "orders"]).out) as { QueueUrl: string };
  aws(endpoint, ["sqs", "create-queue", "--queue-name", "payments"]);
  aws(endpoint, ["sqs", "send-message", "--queue-url", made.QueueUrl, "--message-body", "one"]);
  aws(endpoint, ["sqs", "receive-message", "--queue-url", made.QueueUrl, "--visibility-timeout", "300"]);
  const m = (name: string, services: string[], operations: string[], resources: string[], effect: unknown, share: unknown = { kind: "every" }) => ({ name, matcher: { services, operations, regions: [], callers: [], resources }, effect, share, ends_in_secs: 900 });
  await post("/api/v1/lab/rules", m("Orders are throttled", ["sqs"], ["SendMessage"], ["orders"], { kind: "fail", error: "throttle" }, { kind: "nth", n: 4 }));
  await post("/api/v1/lab/rules", m("Slow queue listing", ["sqs"], ["ListQueues"], [], { kind: "slow", delay_ms: 600, jitter_ms: 200 }));
  await post("/api/v1/lab/rules", m("DynamoDB is down", ["dynamodb"], [], [], { kind: "fail", error: "unavailable" }, { kind: "percent", percent: 30 }));
  const old = await post("/api/v1/lab/rules", m("Payments refuse", ["sqs"], ["SendMessage"], ["payments"], { kind: "fail", error: "access-denied" }));
  for (let i = 0; i < 8; i++) aws(endpoint, ["sqs", "send-message", "--queue-url", `${endpoint}/111122223333/orders`, "--message-body", String(i)]);
  aws(endpoint, ["sqs", "list-queues"]);
  await post(`/api/v1/lab/rules/${old.rule.id}/remove`, {});
  await post("/api/v1/lab/once/expire-visibility", { choice: "orders" });
  await post("/api/v1/lab/once/rest-service", { choice: "sqs", time_wake: true });
  await post("/api/v1/lab/rules", m("Drop queue URL lookups", ["sqs"], ["GetQueueUrl"], [], { kind: "drop" }));
  aws(endpoint, ["sqs", "get-queue-url", "--queue-name", "orders"]);
}

async function settle(p: Page) {
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(500);
}

await seed();
for (const name of only.split(",") as (keyof typeof engines)[]) {
  const b = await engines[name].launch();
  mkdirSync(out, { recursive: true });
  for (const mode of ["light", "dark"] as const) {
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
    const p = await ctx.newPage();
    const shot = async (what: string) => (await settle(p), await p.screenshot({ path: `${out}/rules-${name}-${what}.${mode}.png`, fullPage: what === "rehearse" || what === "live" }));
    await p.goto(`${url}/lab`);
    await p.getByTestId("rules-in-force").waitFor();
    await p.getByTestId("lab-recipe-cards").waitFor();
    await shot("rehearse");
    await p.getByTestId("rehearsal-throttle").getByTestId("rule-start").click();
    await p.getByTestId("rule-dialog").waitFor();
    await p.getByRole("combobox", { name: "Services", exact: true }).click();
    await p.locator("[cmdk-input]").fill("sqs");
    await p.keyboard.press("Enter");
    await p.keyboard.press("Escape");
    await p.getByRole("combobox", { name: "Operations", exact: true }).click();
    await p.locator("[cmdk-input]").fill("ReceiveMessage");
    await p.keyboard.press("Enter");
    await p.keyboard.press("Escape");
    await p.getByTestId("rule-lesson").locator("summary").click();
    await p.getByTestId("rule-sentence-text").filter({ hasText: "Throttle" }).waitFor();
    await shot("dialog");
    await p.keyboard.press("Escape");
    await p.getByTestId("rehearsal-slow").getByTestId("rule-start").click();
    await p.getByTestId("rule-ends").fill("ten");
    await p.getByTestId("problem-ends_in_secs").waitFor();
    await shot("dialog-problem");
    await p.keyboard.press("Escape");
    await p.getByTestId("rule-open").first().click();
    await p.getByTestId("rule-sheet").waitFor();
    await shot("sheet");
    await p.keyboard.press("Escape");
    await p.getByTestId("rehearsal-expire-visibility").getByTestId("once-open").click();
    await p.getByTestId("once-preview").waitFor();
    await p.getByTestId("once-lesson").locator("summary").click();
    await shot("once-form");
    await p.getByTestId("once-run").click();
    await p.getByTestId("once-result").waitFor();
    await shot("once-result");
    await p.keyboard.press("Escape");
    await p.getByTestId("rehearsal-rest-service").getByTestId("once-open").click();
    await p.getByTestId("once-option").waitFor();
    await shot("once-rest");
    // Live, the calls the rules touched (tagged), and the one that got no answer.
    await p.goto(`${url}/live?lab=any`);
    await p.locator('[data-slot="lab-tag"]').first().waitFor();
    await shot("live");
    await ctx.close();
  }
  await b.close();
}
