// The Live boards (LiveA, LiveB, LiveBLoad, LiveC, and the Vanity board's Live), reached against a real doze-aws whose apps have
// called as Go, Python and the AWS CLI, and photographed light and dark in Chromium.
import { expect, test } from "./fixtures";
import { cli, go, serveWorld, shot, world } from "./home-kit";
import { as } from "./home-seed";

serveWorld(1);

const node = (page: import("@playwright/test").Page, id: string) => page.locator(`[data-node="${id}"]`);

test("LiveA: apps on the left, the resources they called and what those are wired to, a panel for the one picked", async ({ page }) => {
  await go(page, "/live", "Live");
  const map = page.getByTestId("resource-map");
  await expect(map).toBeVisible();
  // Each app by its name (an IAM user), each resource by its name with what it holds now.
  for (const app of ["fulfilment", "payments", "catalog", "report-job", "admin-cli"]) await expect(map.locator('[data-testid="map-app"]', { hasText: app })).toHaveCount(1);
  await expect(node(page, "sqs/queues/pick-lists")).toContainText("3 waiting · 2 in flight");
  await expect(node(page, "sqs/queues/pick-lists-dlq")).toContainText("1 dead letter");
  await expect(node(page, "eventbridge/buses/larder")).toContainText("1 rule · 1 target");
  await expect(node(page, "s3/buckets/product-images")).toContainText("notifies 1 target");
  // The wires: a rule's target, the redrive with its limit, a bucket's notification.
  const labels = page.getByTestId("wire-label");
  await expect(labels.filter({ hasText: "target" })).toContainText("payment-settled");
  await expect(labels.filter({ hasText: "redrive" })).toContainText("after 5 receives");
  await expect(labels.filter({ hasText: "notify" })).toContainText("ObjectCreated");
  // A rule's target, the redrive, a notification, and the topic's two subscriptions.
  await expect(page.getByTestId("map-wire")).toHaveCount(5);
  // Not wired: what exists and nothing calls or feeds.
  await page.getByTestId("not-wired").locator("summary").click();
  await expect(page.getByTestId("not-wired-item").filter({ hasText: "deliveries" })).toHaveCount(1);
  // The panel of a queue: what it holds, who feeds it, where it dead-letters to, who reads it, and its verbs.
  await node(page, "sqs/queues/pick-lists").click();
  const panel = page.getByTestId("node-panel");
  await expect(panel.getByTestId("panel-title")).toHaveText("pick-lists");
  await expect(panel.getByTestId("panel-sentence")).toHaveText("3 waiting · 2 in flight");
  await expect(panel).toContainText("Fed by");
  await expect(panel).toContainText("larder");
  await expect(panel).toContainText("Dead letters to");
  await expect(panel).toContainText("pick-lists-dlq");
  await expect(panel).toContainText("after 5 receives");
  await expect(panel).toContainText("Read by");
  await expect(panel.getByTestId("node-spark")).toBeVisible();
  await expect(panel.getByTestId("panel-send")).toHaveAttribute("href", /\/resources\/sqs\/queues\/pick-lists\?view=messages/);
  await expect(panel.getByTestId("panel-open")).toHaveAttribute("href", "/resources/sqs/queues/pick-lists");
  await expect(page).toHaveURL(/node=sqs%2Fqueues%2Fpick-lists/);
  await expect(page.getByText("The number is messages waiting. In-flight ones are hidden, not gone.")).toBeVisible();
  // A name in the panel picks that node.
  await panel.getByTestId("panel-link").filter({ hasText: "pick-lists-dlq" }).click();
  await expect(panel.getByTestId("panel-title")).toHaveText("pick-lists-dlq");
  await expect(panel).toContainText("Dead-letter queue of");
  await panel.getByTestId("panel-close").click();
  await expect(panel).toHaveCount(0);
  await node(page, "sqs/queues/pick-lists").click();
  await shot(page, "LiveA");
});

test("LiveB: opening a call lights its route from the app to what it named and on along the wiring", async ({ page }) => {
  await go(page, "/live", "Live");
  // payments' SendMessage to pick-lists, which dead-letters to pick-lists-dlq.
  const row = page.locator("tr[data-row]", { hasText: "SendMessage" }).filter({ hasText: "payments" }).first();
  await row.click();
  await expect(page.getByTestId("call-sheet")).toBeVisible();
  const lit = page.locator('[data-testid="map-line"][stroke-width="3.5"], [data-testid="map-wire"][stroke-width="3.5"]');
  await expect(lit).toHaveCount(2);
  await expect(node(page, "sqs/queues/pick-lists")).toHaveAttribute("aria-pressed", "true");
  await expect(node(page, "sqs/queues/pick-lists-dlq")).toHaveAttribute("aria-pressed", "true");
  await shot(page, "LiveB");
});

test("LiveBLoad: what needs a look, a function that failed, Focus, and why a call would be denied against the policy the caller has", async ({ page }) => {
  await go(page, "/live", "Live");
  const strip = page.getByTestId("needs-look");
  // One chip a kind: a function that failed, soft IAM, a refusal, an operation not built; and two kinds of what is stored.
  await expect(strip.getByTestId("look")).toHaveCount(6);
  await expect(strip.locator('[data-word="FUNCTION ERROR"]')).toContainText("Invoke");
  await expect(strip.locator('[data-word="FUNCTION ERROR"]')).toContainText("Unhandled");
  await expect(strip.locator('[data-word="WOULD BE DENIED (soft)"]')).toContainText("report-job");
  await expect(strip.locator('[data-word="REFUSED"]')).toContainText("CreateQueue");
  await expect(strip.locator('[data-word="NOT BUILT"]')).toContainText("SelectObjectContent");
  await expect(strip.locator('[data-word="NOT ACTIVE LOCALLY"]').filter({ hasText: "1 subscription stored" })).toHaveCount(1);
  await expect(strip.locator('[data-word="NOT ACTIVE LOCALLY"]').filter({ hasText: "1 domain name stored" })).toHaveCount(1);
  // The wire says it too: the invocation answered 200, the function did not work; and the node of the function carries the mark.
  const failed = page.getByRole("table", { name: "Calls, newest first" }).locator("tr[data-row]", { hasText: "Invoke" }).filter({ has: page.getByTestId("function-error") });
  await expect(failed.first()).toContainText("FUNCTION ERROR");
  await expect(failed.first()).toContainText("Unhandled");
  await expect(page.locator('[data-node="lambda/functions/resize-product-image"] [aria-label="1 function error"]')).toHaveCount(1);
  // Focus: active in the last five minutes, or every flow (both are the whole world just after the apps called).
  await expect(page.getByTestId("focus-active")).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("focus-all").click();
  await expect(page).toHaveURL(/focus=all/);
  await page.getByTestId("focus-active").click();
  // The would-be-denied call opens with how many statements match, the policies the caller has, and what would be added to the one it has.
  await strip.locator('[data-word="WOULD BE DENIED (soft)"]').click();
  const explain = page.getByTestId("explain");
  await expect(explain).toContainText("Why this would be denied");
  await expect(explain.getByTestId("explain-count")).toContainText("0 statements match this resource");
  await expect(explain.getByTestId("explain-caller")).toContainText("report-job");
  await expect(explain.getByTestId("explain-action")).toHaveText("s3:GetObject");
  await expect(explain.getByTestId("explain-resource")).toContainText("arn:aws:s3:::receipts/r-8841.pdf");
  await expect(explain).toContainText("IAM soft: allowed and recorded; real AWS would deny");
  const policy = explain.locator('[data-testid="explain-policy"][data-name="report-job-readonly"]');
  await expect(policy).toContainText("identity policy");
  await expect(policy).toContainText("allows s3:GetObject, s3:PutObject, s3:ListBucket on arn:aws:s3:::reports/*, arn:aws:s3:::reports");
  await expect(policy).toContainText("no statement matches this resource");
  await expect(explain.getByTestId("explain-adds")).toContainText("+ s3:GetObject on arn:aws:s3:::receipts/*");
  await expect(explain.getByTestId("explain-target")).toContainText("report-job-readonly");
  await expect(explain).toContainText("Soft mode never blocks. It records what real AWS would have said.");
  await shot(page, "LiveBLoad");
  await explain.getByTestId("explain-add").click();
  await expect(page.getByTestId("pass-dialog")).toBeVisible();
  await expect(page.getByTestId("pass-adds")).toContainText("+ s3:GetObject on arn:aws:s3:::receipts/*");
  await expect(page.getByTestId("policy-name")).toHaveValue("report-job-readonly");
  // The whole policy is the one the caller has, with the new statement after its own.
  await expect(page.getByTestId("drafted-policy")).toContainText('"arn:aws:s3:::reports/*"');
  await expect(page.getByTestId("drafted-policy")).toContainText('"arn:aws:s3:::receipts/*"');
  await page.keyboard.press("Escape");
});

test("a refused call says which rule fired, with the service's own words, and what would pass", async ({ page }) => {
  await go(page, "/live", "Live");
  await page.locator('[data-testid="look"][data-word="REFUSED"]').click();
  const refused = page.getByTestId("refused");
  await expect(refused).toContainText("Why it was refused");
  await expect(refused.getByTestId("refused-message")).not.toBeEmpty();
  await expect(refused).toContainText("InvalidParameterValue");
  await expect(refused.getByTestId("refused-pass")).toContainText("Change the one the message names");
  await expect(page.getByTestId("no-replay")).toContainText("Replay is not available: the record keeps no request bodies");
  await expect(page.getByTestId("call-summary")).toContainText("the caller's to fix");
});

test("the wire says what came back, keeps a rule's name, and does not claim hours of history", async ({ page }) => {
  await go(page, "/live", "Live");
  const wire = page.getByRole("table", { name: "Calls, newest first" });
  // A receive says how many messages came back, a send the size of what it sent, a publish how many deliveries SNS made.
  await expect(wire.getByTestId("answer-words").filter({ hasText: /^· (no messages|\d+ messages?)$/ }).first()).toBeVisible();
  await expect(wire.getByTestId("answer-words").filter({ hasText: /^· sent \d+ B$/ }).first()).toBeVisible();
  await expect(wire.locator("tr[data-row]", { hasText: "Publish" }).getByTestId("answer-words").filter({ hasText: /^· delivered to \d+/ }).first()).toBeVisible();
  // An operation the record reads nothing of keeps the size of its answer.
  await expect(wire.getByTestId("answer-words").filter({ hasText: /^· \d+ B$/ }).first()).toBeVisible();
  // The drawer of a publish says it again, with what it is read from.
  await wire.locator("tr[data-row]", { hasText: "Publish" }).getByTestId("open-call").first().click();
  await expect(page.getByTestId("call-did")).toContainText("delivered to");
  await expect(page.getByTestId("call-did")).toContainText("The record keeps no bodies");
  await page.keyboard.press("Escape");
  // The rule that delivered a message is named, marked as a rule.
  const viaRule = wire.locator("tr[data-row]", { hasText: "pick-lists" }).filter({ hasText: "SendMessage" }).filter({ has: page.getByTestId("via-pill") }).first();
  await expect(viaRule).toContainText("payment-settled");
  await expect(viaRule.getByTestId("via-pill")).toHaveText("rule");
  // The page says what it keeps.
  await expect(page.locator("main")).toContainText("Kept in memory: the newest 1,024 calls");
  await expect(page.locator("main")).toContainText("not hours of history");
  await expect(page.getByTestId("rail-footer")).toContainText("jump or do");
});

test("?app= is the caller by its name", async ({ page }) => {
  await go(page, "/live?view=wire&app=payments", "Live");
  const wire = page.getByRole("table", { name: "Calls, newest first" });
  await expect(wire.locator("tr[data-row]").first()).toContainText("payments");
  const rows = await wire.locator("tr[data-row]").allTextContents();
  expect(rows.length).toBeGreaterThan(1);
  for (const r of rows) expect(r).toContain("payments");
});

test("LiveC: the same map in columns by domain, and what REFUSED and WOULD BE DENIED are", async ({ page }) => {
  await go(page, "/live?layout=domain", "Live");
  const heads = page.getByTestId("domain-head");
  await expect(heads).toHaveText(["Apps", "Messaging and events", "Storage and compute"]);
  const col = async (id: string) => (await node(page, id).boundingBox())!.x;
  expect(await col("sqs/queues/pick-lists")).toBeGreaterThan(await col("app|" + world.apps.payments));
  expect(await col("s3/buckets/product-images")).toBeGreaterThan(await col("sqs/queues/pick-lists"));
  // The wire's words: REFUSED is the service refusing the request, WOULD BE DENIED is soft IAM.
  const wire = page.getByRole("table", { name: "Calls, newest first" });
  await expect(wire.locator("tr[data-row]", { hasText: "CreateQueue" }).first()).toContainText("REFUSED");
  await expect(wire.locator("tr[data-row]", { hasText: "GetObject" }).first()).toContainText(/would be denied/i);
  await shot(page, "LiveC");
});

test("Vanity: the live space as the verdicts say it is built: wire, copy as, the map with caller by app, one number per resource", async ({ page }) => {
  await go(page, "/live", "Live");
  // Copy as AWS CLI and curl are on every call, and Open resource.
  await page.locator("tr[data-row]", { hasText: "ReceiveMessage" }).first().click();
  await expect(page.getByTestId("call-copy-cli")).toBeEnabled();
  await expect(page.getByTestId("call-copy-curl")).toBeEnabled();
  await expect(page.getByTestId("call-open-resource")).toContainText("pick-lists");
  // No Replay anywhere: the record keeps no request bodies.
  await expect(page.getByRole("button", { name: /replay/i })).toHaveCount(0);
  await page.keyboard.press("Escape");
  // The one number per resource: a node's live sentence; the minutes the record holds, said as they are.
  await expect(node(page, "sqs/queues/pick-lists")).toContainText("waiting");
  await expect(page.getByTestId("timeline")).toContainText("calls held");
  await shot(page, "Vanity");
});

test("the map of services is still there: Services draws who called which service", async ({ page }) => {
  await go(page, "/live?by=service", "Live");
  await expect(page.getByTestId("live-map")).toBeVisible();
  await expect(page.getByTestId("map-line").first()).toBeVisible();
  await page.getByTestId("by-resource").click();
  await expect(page.getByTestId("resource-map")).toBeVisible();
});

test("a busy world: the busiest flows are drawn, the rest are counted as quieter, and All flows draws them", async ({ page }) => {
  for (let i = 0; i < 14; i++) {
    cli(["sqs", "create-queue", "--queue-name", `busy-${i}`]);
    cli(["sqs", "send-message", "--queue-url", `${world.server.endpoint}/${world.server.account}/busy-${i}`, "--message-body", "x"]);
  }
  for (const k of [world.apps.payments, world.apps.catalog]) for (let i = 0; i < 14; i++) as(world.server.endpoint, k, ["sqs", "get-queue-url", "--queue-name", `busy-${i}`]);
  await go(page, "/live", "Live");
  const map = page.getByTestId("resource-map");
  await expect(map).toBeVisible();
  const drawn = await map.locator('[data-testid="map-resource"]').count();
  expect(drawn).toBeLessThanOrEqual(24);
  const height = (await map.boundingBox())!.height;
  expect(height).toBeLessThan(1000);
  await expect(page.getByTestId("quiet-flows")).toContainText("quieter flows");
  await shot(page, "LiveBLoad.busy");
  await page.getByTestId("focus-all").click();
  expect(await map.locator('[data-testid="map-resource"]').count()).toBeGreaterThan(drawn);
  // No label sits over a card: a label's box does not meet any card's.
  const labels = await page.getByTestId("wire-label").evaluateAll((els) => els.map((e) => e.getBoundingClientRect()));
  const cards = await page.locator('[data-testid="map-resource"], [data-testid="map-app"]').evaluateAll((els) => els.map((e) => e.getBoundingClientRect()));
  for (const l of labels) for (const c of cards) expect(l.left < c.right && l.right > c.left && l.top < c.bottom && l.bottom > c.top, "a label over a card").toBe(false);
});

test("Add to policy edits the policy the caller has: the same name, its statements kept, the new one after them", async ({ page }) => {
  await go(page, "/live", "Live");
  await page.locator('[data-testid="look"][data-word="WOULD BE DENIED (soft)"]').click();
  await page.getByTestId("explain-add").click();
  await expect(page.getByTestId("pass-dialog")).toBeVisible();
  await page.getByTestId("attach-submit").click();
  await expect(page.getByTestId("pass-dialog")).toHaveCount(0);
  // IAM holds the one policy, saved over, and no policy of another name was made.
  const names = JSON.parse(cli(["iam", "list-user-policies", "--user-name", "report-job"])) as { PolicyNames: string[] };
  expect(names.PolicyNames).toEqual(["report-job-readonly"]);
  const doc = JSON.parse(cli(["iam", "get-user-policy", "--user-name", "report-job", "--policy-name", "report-job-readonly"])) as { PolicyDocument: { Statement: { Sid?: string; Resource: string | string[] }[] } };
  expect(doc.PolicyDocument.Statement.length).toBe(2);
  expect(JSON.stringify(doc.PolicyDocument.Statement[0])).toContain("arn:aws:s3:::reports/*");
  expect(doc.PolicyDocument.Statement[1]?.Resource).toBe("arn:aws:s3:::receipts/*");
  expect(doc.PolicyDocument.Statement[1]?.Sid).toBe("AllowGetObjectreceipts");
});
