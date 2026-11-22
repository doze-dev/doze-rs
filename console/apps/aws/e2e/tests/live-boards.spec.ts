// The Live boards (LiveA, LiveB, LiveBLoad, LiveC, and the Vanity board's Live), reached against a real doze-aws whose apps have
// called as Go, Python and the AWS CLI, and photographed light and dark in Chromium.
import { expect, test } from "./fixtures";
import { go, serveWorld, shot, world } from "./home-kit";

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

test("LiveBLoad: what needs a look, Focus, and why a call would be denied with Add to policy", async ({ page }) => {
  await go(page, "/live", "Live");
  const strip = page.getByTestId("needs-look");
  await expect(strip.getByTestId("look")).toHaveCount(4);
  // What a script made and doze-aws keeps without acting on it is counted with them.
  await expect(strip.locator('[data-word="NOT ACTIVE LOCALLY"]')).toContainText("1 subscription stored");
  await expect(strip.locator('[data-word="WOULD BE DENIED (soft)"]')).toContainText("report-job");
  await expect(strip.locator('[data-word="REFUSED"]')).toContainText("CreateLogGroup");
  await expect(strip.locator('[data-word="NOT BUILT"]')).toContainText("SelectObjectContent");
  // Focus: active in the last five minutes, or every flow (both are the whole world just after the apps called).
  await expect(page.getByTestId("focus-active")).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("focus-all").click();
  await expect(page).toHaveURL(/focus=all/);
  await page.getByTestId("focus-active").click();
  // The would-be-denied call opens with the reason and what Add to policy would add.
  await strip.locator('[data-word="WOULD BE DENIED (soft)"]').click();
  const explain = page.getByTestId("explain");
  await expect(explain).toContainText("Why this would be denied");
  await expect(explain.getByTestId("explain-caller")).toContainText("report-job");
  await expect(explain.getByTestId("explain-action")).toHaveText("s3:GetObject");
  await expect(explain.getByTestId("explain-resource")).toContainText("arn:aws:s3:::receipts/r-8841.pdf");
  await expect(explain).toContainText("IAM soft: allowed and recorded; real AWS would deny");
  await expect(explain.getByTestId("explain-adds")).toContainText("+ s3:GetObject on arn:aws:s3:::receipts/r-8841.pdf");
  await expect(explain).toContainText("Soft mode never blocks. It records what real AWS would have said.");
  await shot(page, "LiveBLoad");
  // Add to policy opens the same dialog as Make this pass on Access, and puts the statement on the user.
  await explain.getByTestId("explain-add").click();
  await expect(page.getByTestId("pass-dialog")).toBeVisible();
  await expect(page.getByTestId("drafted-policy")).toContainText("s3:GetObject");
  await page.keyboard.press("Escape");
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
