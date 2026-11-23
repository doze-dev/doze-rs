// The AWS console's kit, create flows: the Stepper, the CallPanel (live, from a preview the page answers in place), FormPage
// with its fields and refusals, the wizard and its review, the key-value rows, the policy editor and the pickers.
import { type Locator } from "@playwright/test";
import { expect, open, overflowX, ringOf, tabTo, test } from "./fixtures";

const LOOK = { theme: "aws", mode: "light" } as const;
const DARK = { theme: "aws", mode: "dark" } as const;

test("create pages: no sideways scroll at 1024 px, in either look", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  for (const look of [LOOK, DARK, { theme: "kafka", mode: "light" } as const]) {
    for (const path of ["/create", "/fields"]) {
      await open(page, path, look);
      await page.waitForTimeout(400);
      expect(await overflowX(page), `${path} under ${look.theme}-${look.mode}`).toBe(0);
    }
  }
});

test("Stepper: a passed step goes back, the current one says so, a step not reached is no button", async ({ page }) => {
  await open(page, "/create", LOOK);
  const demo = page.getByTestId("stepper-demo");
  const step = (id: string) => demo.locator(`li[data-step="${id}"]`);
  await expect(step("rotation").locator("[aria-current=step]")).toHaveCount(1);
  await expect(step("review").getByRole("button")).toHaveCount(0);
  await expect(step("type").getByRole("button")).toHaveCount(1);
  await step("type").getByRole("button").click();
  await expect(step("type").locator("[aria-current=step]")).toHaveCount(1);
  // The steps already reached can be walked forward again.
  await expect(step("rotation").getByRole("button")).toHaveCount(1);
  await demo.getByTestId("stepper-next").click();
  await expect(step("configure").locator("[aria-current=step]")).toHaveCount(1);
});

test("CallPanel: live from the preview, the copy buttons, what is needed, a refusal, a failure, loading", async ({ page }) => {
  await open(page, "/create", LOOK);
  const live = page.getByTestId("live-panel");
  await expect(live.getByTestId("live-panel-code")).toContainText('"QueueName": "pick-lists"');
  await expect(live.getByText("Copy as", { exact: true })).toBeVisible();
  await expect(live.getByRole("button", { name: "AWS CLI" })).toBeVisible();
  await expect(live.getByRole("button", { name: "boto3" })).toBeVisible();
  await expect(live.getByRole("button", { name: "Copy JSON" })).toBeVisible();
  await expect(live.locator("[role=tab]")).toHaveCount(0);
  await page.getByTestId("live-name").fill("");
  await expect(live.getByTestId("live-panel-missing")).toHaveText("Needs QueueName.");
  await page.getByTestId("live-name").fill("orders");
  await expect(live.getByTestId("live-panel-code")).toContainText('"QueueName": "orders"');
  await expect(live.getByTestId("live-panel-missing")).toHaveCount(0);

  await expect(page.getByTestId("panel-refused")).toHaveAttribute("data-refused", "");
  await expect(page.getByTestId("panel-refused-came-back")).toContainText("QueueNameExists");
  await expect(page.getByTestId("panel-refused").getByRole("heading")).toHaveText("The call that was refused");
  await expect(page.getByTestId("panel-idle-idle")).toBeVisible();
  await expect(page.getByTestId("panel-unbuilt-error")).toContainText("has not built Unbuilt");
  await expect(page.getByTestId("panel-unbuilt-error")).toContainText("NOT_BUILT");
  await expect(page.getByTestId("panel-loading-loading")).toBeVisible();
});

test("CallPanel: the preview is asked once the typing pauses, not on every key", async ({ page }) => {
  await open(page, "/create", LOOK);
  await expect(page.getByTestId("live-panel").getByTestId("live-panel-code")).toBeVisible();
  const count = () => page.evaluate(() => (window as unknown as { __mock: { calls: { path: string; body: unknown }[] } }).__mock.calls.filter((c) => c.path === "/api/v1/preview" && JSON.stringify(c.body).includes("pick-lists-")).length);
  expect(await count()).toBe(0);
  await page.getByTestId("live-name").focus();
  await page.keyboard.press("End");
  await page.keyboard.type("-abcdef", { delay: 20 });
  await expect(page.getByTestId("live-panel-code")).toContainText("pick-lists-abcdef");
  expect(await count()).toBeLessThanOrEqual(2);
});

test("CallPanel: Copy puts the AWS CLI line on the clipboard", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "only Chromium grants the clipboard to the run");
  await open(page, "/create", LOOK);
  const live = page.getByTestId("live-panel");
  await expect(live.getByTestId("live-panel-code")).toBeVisible();
  await live.getByRole("button", { name: "AWS CLI" }).last().click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain("aws sqs create-queue --queue-name pick-lists");
});

test("FormPage: the button says why it is off, the call follows the form, and a submit shows its result", async ({ page }) => {
  await open(page, "/create", LOOK);
  const form = page.getByTestId("form-live");
  const submit = form.getByTestId("form-submit");
  await expect(submit).toBeDisabled();
  await expect(form.getByTestId("disabled-why")).toContainText("Give the queue a name");
  await form.getByTestId("queue-name").fill("stock-moves");
  await expect(submit).toBeEnabled();
  await expect(form.getByTestId("disabled-why")).toHaveCount(0);
  await form.getByTestId("queue-visibility").fill("60");
  await expect(form.getByTestId("call-panel-code")).toContainText('"VisibilityTimeout": "60"');
  // A shut section says what it holds, and opens.
  await expect(form.getByTestId("section-Encryption-summary")).toContainText("Enabled with an Amazon SQS key");
  await form.getByTestId("section-Encryption-toggle").click();
  await expect(form.getByText("Stored, not applied locally.")).toBeVisible();
  await submit.click();
  await expect(form.getByTestId("result-banner")).toContainText("Queue stock-moves created.");
  await form.getByTestId("result-banner-dismiss").click();
  await expect(form.getByTestId("result-banner")).toHaveCount(0);
});

test("FormPage: a refusal marks the field, says why and what would work, and shows what came back", async ({ page }) => {
  await open(page, "/create", LOOK);
  const form = page.getByTestId("form-refused");
  await expect(form.getByRole("alert").first()).toContainText("Queue not created.");
  await expect(form.locator('[data-slot="field"][data-refused]')).toHaveCount(1);
  await expect(form.locator('[data-slot="field-refusal"]')).toContainText("A queue with this name already exists, with different attributes. Open pick-lists");
  await expect(form.getByTestId("call-panel-came-back")).toContainText("QueueNameExists");
  const border = await form.getByTestId("queue-name").evaluate((el) => getComputedStyle(el).borderColor);
  const ok = await page.getByTestId("form-live").getByTestId("queue-name").evaluate((el) => getComputedStyle(el).borderColor);
  expect(border).not.toBe(ok);
});

test("wizard: Next through four steps, the review lists every answer, Edit goes back to its step", async ({ page }) => {
  await open(page, "/create", LOOK);
  const w = page.getByTestId("wizard");
  await expect(w.getByText("Step 1 of 4.")).toBeVisible();
  await expect(w.getByTestId("form-back")).toHaveCount(0);
  await w.getByTestId("form-submit").click();
  await w.getByRole("textbox").fill("larder/prod/other-key");
  await w.getByTestId("form-submit").click();
  await w.getByTestId("form-submit").click();
  await expect(w.getByText("Step 4 of 4.")).toBeVisible();
  await expect(w.getByTestId("form-submit")).toHaveText("Store");
  await expect(w.getByLabel("Edit Configure secret")).toBeVisible();
  await expect(w.getByText("larder/prod/other-key").first()).toBeVisible();
  await expect(w.getByTestId("call-panel-code")).toContainText("larder/prod/other-key");
  await w.getByLabel("Edit Configure secret").click();
  await expect(w.getByText("Step 2 of 4.")).toBeVisible();
  await expect(w.locator('li[data-step="configure"] [aria-current=step]')).toHaveCount(1);
  // The review is still reachable by the step it came from.
  await expect(w.locator('li[data-step="review"]').getByRole("button")).toHaveCount(1);
  await w.getByTestId("form-back").click();
  await expect(w.getByText("Step 1 of 4.")).toBeVisible();
});

test("KeyValueEditor: add, remove, empty and repeated keys are said once the field is left, the limit stops adding", async ({ page }) => {
  await open(page, "/fields", LOOK);
  const sec = page.getByTestId("section-kv");
  const filled = sec.getByTestId("kv-2-false-0");
  await filled.getByTestId("kv-2-false-0-add").click();
  await expect(filled.getByTestId("kv-2-false-0-key-2")).toBeVisible();
  await filled.getByTestId("kv-2-false-0-key-2").fill("env");
  // A repeated key is said at once, on both rows: it is in what was typed.
  await expect(filled.getByText("Another row uses this key.")).toHaveCount(2);
  await filled.getByTestId("kv-2-false-0-remove-2").click();
  await expect(filled.getByText("Another row uses this key.")).toHaveCount(0);
  // A new blank row is not wrong; a value with no key is, once the field has been left.
  await filled.getByTestId("kv-2-false-0-add").click();
  await filled.getByTestId("kv-2-false-0-key-2").focus();
  await filled.getByTestId("kv-2-false-0-value-2").focus();
  await expect(filled.getByText("Enter a key.")).toHaveCount(0);
  await filled.getByTestId("kv-2-false-0-value-2").fill("x");
  await filled.getByTestId("kv-2-false-0-key-2").focus();
  await expect(filled.getByText("Enter a key.")).toHaveCount(1);
  // Errors the page asked to show are shown at once.
  const shown = sec.getByTestId("kv-3-true-0");
  await expect(shown.getByText("Another row uses this key.")).toHaveCount(2);
  await expect(shown.getByText("Enter a key.")).toHaveCount(1);
  expect(await shown.getByTestId("kv-3-true-0-key-0").getAttribute("aria-invalid")).toBe("true");
  // The limit.
  const full = sec.getByTestId("kv-2-false-2");
  await expect(full.getByTestId("kv-2-false-2-add")).toBeDisabled();
  await expect(full).toContainText("The most is 2.");
});

test("PolicyEditor: what a policy says in words, what is wrong with it, and nothing invented for text that is not JSON", async ({ page }) => {
  await open(page, "/fields", LOOK);
  const id = page.getByTestId("policy-identity");
  await expect(id.getByTestId("policy-identity-statement")).toHaveCount(2);
  await expect(id.getByTestId("policy-identity-statement").first()).toContainText("Allow");
  await expect(id.getByTestId("policy-identity-statement").nth(1)).toContainText("with a condition");
  await expect(id.getByTestId("policy-identity-problems")).toHaveCount(0);
  // Editing is checked as it is typed.
  await id.locator("textarea").fill("{}");
  await expect(id.getByTestId("policy-identity-problems")).toContainText("It has no Statement.");
  await id.locator("textarea").fill("{ nope");
  await expect(id.getByTestId("policy-identity-problems")).toHaveCount(0);
  await expect(id.getByTestId("policy-identity-statements")).toHaveCount(0);
  const res = page.getByTestId("policy-resource");
  await expect(res.getByTestId("policy-resource-statement")).toContainText("for Service: sns.amazonaws.com");
});

test("pickers: seven kinds list what the console reads, one is chosen, an empty or failed read says so", async ({ page }) => {
  await open(page, "/fields", LOOK);
  const sec = page.getByTestId("section-pickers");
  const queue = sec.getByRole("combobox", { name: "Choose a queue" });
  await expect(queue).toContainText("Choose a queue");
  await queue.click();
  await expect(page.getByRole("option")).toHaveCount(6);
  await page.getByRole("option", { name: "pick-lists-dlq" }).click();
  await expect(queue).toContainText("pick-lists-dlq");
  await expect(sec.getByRole("combobox", { name: "Choose a topic" })).toBeVisible();
  await expect(sec.getByRole("combobox", { name: "Choose an event bus" })).toContainText("larder");
  await expect(sec.getByText("There are no queues yet").first()).toBeVisible();
  await expect(sec.getByTestId("picker-error")).toContainText("The queues could not be read.");
});

test("keyboard: a section's toggle, the call panel's copy buttons and a section take the focus with a ring", async ({ page, browserName }) => {
  await open(page, "/create", LOOK);
  const tabs: Locator[] = [page.getByTestId("live-panel").getByRole("button", { name: "AWS CLI" }), page.getByTestId("live-panel").getByRole("button", { name: "boto3" }), page.getByTestId("live-panel").getByRole("button", { name: "Copy JSON" }), page.getByTestId("form-live").getByTestId("section-Encryption-toggle")];
  for (const t of tabs) {
    await tabTo(page, browserName, t, 200);
    const ring = await ringOf(t);
    expect(ring.visible, `${await t.textContent()} shows a ring`).toBe(true);
  }
});
