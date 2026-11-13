// Slice R1b: Create topic (a page), and the write overlays of a topic (Produce, Add partitions, Delete records, Delete
// topic), driven with the mouse and the keyboard; every write is checked by what the broker then says.
import { choose, expect, overflowX, test } from "./fixtures";
import { call, fresh, get, produce, setConfig, topic } from "./r1b-helpers";

const dialogUrl = (t: string, d: string, extra = "") => `/topics/${encodeURIComponent(t)}?dialog=${d}${extra}`;

test.describe("Create topic", () => {
  test("three steps, the server's live check, Check only, then Create: the topic is there", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "made");
    await page.goto("/topics/new");
    await expect(page.locator("h1")).toHaveText("Create topic");
    await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Topics");
    const nameField = page.getByTestId("name");
    await expect(nameField).toBeFocused();
    await page.keyboard.type(name);
    // The partition count is the broker's default, then Kafka's validate_only answers.
    await expect(page.getByTestId("partitions")).toHaveValue(/^\d+$/);
    await expect(page.getByTestId("live-check")).toContainText("Kafka would take this", { timeout: 10_000 });
    await expect(page.getByTestId("the-call")).toContainText(name);
    await page.getByTestId("partitions").fill("3");
    await page.getByTestId("template").getByText("Compacted state").click();
    await expect(page.getByTestId("terminal")).toContainText("cleanup.policy=compact");
    for (const w of [1024, 1440]) {
      await page.setViewportSize({ width: w, height: 900 });
      expect(await overflowX(page), `overflow at ${w}`).toBe(0);
    }
    await page.getByTestId("next-config").click();
    await expect(page).toHaveURL(/step=config/);
    await expect(page.getByTestId("cleanup").getByRole("radio", { name: /^compact/ })).toHaveAttribute("aria-checked", "true");
    await page.getByTestId("retention").fill("3600000");
    await expect(page.getByText("= 1 h").or(page.getByText(/= 1 hour/))).toBeVisible();
    await page.getByTestId("next-review").click();
    await expect(page).toHaveURL(/step=review/);
    await expect(page.getByTestId("review")).toContainText(name);
    // Back returns to the step before, with what was typed.
    await page.goBack();
    await expect(page.getByTestId("retention")).toHaveValue("3600000");
    await page.goForward();
    await page.getByTestId("check-only").click();
    await expect(page.getByTestId("checked")).toContainText("Every check passed for 3 partitions");
    expect((await get(request, `/api/v1/topics/${name}/delete`)).status, "Check only makes nothing").toBe(404);
    await page.getByTestId("create-submit").click();
    await expect(page.getByTestId("created")).toContainText(`Created ${name}.`);
    await expect(page.locator("h1")).toHaveText(name);
    const made = await get(request, `/api/v1/topics/${name}/config`);
    expect(made.status).toBe(200);
    expect(made.body.cleanup).toBe("compact");
    expect(made.body.rows.find((r: { name: string }) => r.name === "retention.ms").value).toBe("3600000");
    await page.getByTestId("created-produce").click();
    await expect(page).toHaveURL(new RegExp(`/topics/${name}\\?dialog=produce`));
    await expect(page.getByTestId("dialog-produce")).toBeVisible();
    void baseURL;
  });

  test("a name Kafka forbids, a name that is taken or collides: said under the field, and Next stays shut", async ({ page }) => {
    await page.goto("/topics/new");
    const nameField = page.getByTestId("name");
    await nameField.fill("bad name!");
    await expect(page.getByTestId("field-error").first()).toContainText("ASCII letters and digits");
    await expect(page.getByTestId("next-config")).toBeDisabled();
    await nameField.fill("orders");
    await expect(page.getByTestId("field-error").first()).toHaveText("A topic has this name already.");
    await expect(page.getByTestId("next-config")).toBeDisabled();
    await nameField.fill("payments_settled");
    await expect(page.getByTestId("field-error").first()).toHaveText("This name collides with the topic payments.settled.");
    await expect(page.getByTestId("next-config")).toBeDisabled();
  });

  test("a topic made while the form is open: Kafka refuses with its own words, and the way on", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "race");
    await page.goto("/topics/new");
    await page.getByTestId("name").fill(name);
    await expect(page.getByTestId("live-check")).toContainText("Kafka would take this", { timeout: 10_000 });
    await page.getByTestId("next-config").click();
    await page.getByTestId("next-review").click();
    await topic(request, baseURL!, name);
    await page.getByTestId("create-submit").click();
    const refused = page.getByTestId("create-refused");
    await expect(refused.getByTestId("refusal-code")).toHaveText("TOPIC_ALREADY_EXISTS (36)");
    await expect(refused.getByTestId("refusal-message")).toContainText(name);
    await expect(refused.getByTestId("the-answer")).toContainText("error_code");
    await refused.getByRole("link", { name: "Open the existing topic" }).click();
    await expect(page).toHaveURL(new RegExp(`/topics/${name}$`));
  });
});

test.describe("Produce", () => {
  test("a keyed record with a header to a chosen partition: the offset the log chose, then another", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "keyed");
    await topic(request, baseURL!, name, 3);
    await page.goto(`/topics/${name}`);
    await page.goto(dialogUrl(name, "produce"));
    const d = page.getByTestId("dialog-produce");
    await expect(d).toBeVisible();
    await choose(page, d.getByTestId("partition"), "2");
    await d.getByTestId("key").fill("ord-1");
    await d.getByTestId("value").fill('{"order_id":"ord-1","qty":2}');
    await expect(d.getByTestId("json-hint")).toContainText("Valid JSON");
    await d.getByTestId("value").fill('{"order_id":');
    await expect(d.getByTestId("json-hint")).toContainText("Not valid JSON");
    await d.getByTestId("value").fill('{"order_id":"ord-1"}');
    await d.getByRole("textbox", { name: "Header name 1" }).fill("content-type");
    await d.getByRole("textbox", { name: "Header value 1" }).fill("application/json");
    await d.getByTestId("add-header").click();
    await expect(d.getByTestId("header-row")).toHaveCount(2);
    await d.getByRole("button", { name: "Remove header 2" }).click();
    await expect(d.getByTestId("header-row")).toHaveCount(1);
    await d.getByTestId("acks").getByRole("radio", { name: "1", exact: true }).click();
    await expect(d.getByTestId("the-call")).toContainText("acks: 1");
    await expect(d.getByTestId("the-call")).toContainText("content-type");
    await expect(d.getByTestId("the-call")).toContainText("index: 2");
    // The teaching folds away; open, it says Kafka's words.
    await d.getByTestId("teaching-toggle").click();
    await expect(d.getByTestId("teach")).toContainText("Kafka says");
    await d.getByTestId("send").click();
    await expect(d.getByTestId("produced")).toContainText(`${name}-2`);
    await expect(d.getByTestId("produced-offset")).toHaveText("0");
    await d.getByTestId("produce-another").click();
    await d.getByTestId("value").fill("second");
    await d.getByTestId("send").click();
    await expect(d.getByTestId("produced-offset")).toHaveText("1");
    // The broker has both: a third lands at offset 2 of the same partition.
    expect(await produce(request, baseURL!, name, "ord-1", "third", "2")).toBe(2);
    await page.keyboard.press("Escape");
    await expect(d).toBeHidden();
    await expect(page).not.toHaveURL(/dialog=/);
  });

  test("a keyless record is accepted on a delete topic, and Back closes the dialog", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "keyless");
    await topic(request, baseURL!, name, 1);
    await page.goto(`/topics/${name}`);
    await page.getByTestId("act-produce").click();
    await expect(page).toHaveURL(/dialog=produce/);
    const d = page.getByTestId("dialog-produce");
    await d.getByTestId("value").fill("no key here");
    await d.getByTestId("send").click();
    await expect(d.getByTestId("produced")).toBeVisible();
    await expect(d.getByTestId("produced-partition")).toHaveText("0");
    await page.goBack();
    await expect(d).toBeHidden();
    await expect(page).toHaveURL(new RegExp(`/topics/${name}$`));
  });

  test("a keyless record to a compacted topic: refused INVALID_RECORD in Kafka's words, the key field says why", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "compacted");
    await topic(request, baseURL!, name, 1, "compact");
    await page.goto(dialogUrl(name, "produce"));
    const d = page.getByTestId("dialog-produce");
    await expect(d.getByText("required on this topic")).toBeVisible();
    await d.getByTestId("value").fill("v");
    await d.getByTestId("send").click();
    await expect(d.getByTestId("refusal-code")).toHaveText("INVALID_RECORD (87)");
    const api = await call(request, baseURL!, `/api/v1/topics/${name}/produce`, { key: "", value: "v", partition: "", acks: "-1" });
    await expect(d.getByTestId("refusal-message")).toHaveText(api.body.message);
    await expect(d.getByTestId("field-error")).toHaveText("A compacted topic needs a key on every record.");
    // With a key it goes.
    await d.getByTestId("key").fill("k");
    await d.getByTestId("send").click();
    await expect(d.getByTestId("produced")).toBeVisible();
  });

  test("a record over max.message.bytes: refused MESSAGE_TOO_LARGE", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "large");
    await topic(request, baseURL!, name, 1);
    await setConfig(request, baseURL!, name, "max.message.bytes", "200");
    await page.goto(dialogUrl(name, "produce"));
    const d = page.getByTestId("dialog-produce");
    await d.getByTestId("key").fill("k");
    await d.getByTestId("value").fill("x".repeat(600));
    await d.getByTestId("send").click();
    await expect(d.getByTestId("refusal-code")).toHaveText("MESSAGE_TOO_LARGE (10)");
    await expect(d.getByTestId("the-answer")).toBeVisible();
  });
});

test.describe("Add partitions", () => {
  test("the new total, what it adds, then the broker's count; fewer is refused in Kafka's words", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "grow");
    await topic(request, baseURL!, name, 1);
    await page.goto(dialogUrl(name, "add-partitions"));
    const d = page.getByTestId("dialog-add-partitions");
    await expect(d.getByTestId("count")).toHaveValue("2");
    await expect(d.getByTestId("adds")).toHaveText(`adds 1: ${name}-1`);
    await d.getByTestId("count").fill("3");
    await expect(d.getByTestId("add-submit")).toHaveText("Add 2 partitions");
    await expect(d.getByTestId("the-call")).toContainText("count: 3");
    await d.getByTestId("add-submit").click();
    await expect(d.getByTestId("added-to")).toHaveText("3");
    expect((await get(request, `/api/v1/topics/${name}/add-partitions`)).body.view.have).toBe(3);
    await d.getByRole("button", { name: "Done" }).click();
    await expect(page).not.toHaveURL(/dialog=/);
    // Fewer than now: Kafka's refusal, in the dialog.
    await page.goto(dialogUrl(name, "add-partitions"));
    await d.getByTestId("count").fill("2");
    await d.getByTestId("add-submit").click();
    await expect(d.getByTestId("refusal-code")).toHaveText("INVALID_PARTITIONS (37)");
    await expect(d.getByTestId("the-answer")).toBeVisible();
  });
});

test.describe("Delete records", () => {
  test("the preview says what goes and the button counts it; the log starts there after", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "trim");
    await topic(request, baseURL!, name, 2);
    for (let i = 0; i < 5; i++) await produce(request, baseURL!, name, `k${i}`, `v${i}`, "1");
    await page.goto(dialogUrl(name, "delete-records", "&partition=0"));
    const d = page.getByTestId("dialog-delete-records");
    // A deep link's partition is shown in the trigger before anything is chosen.
    await expect(d.getByTestId("records-partition")).toHaveText("0");
    await choose(page, d.getByTestId("records-partition"), "1");
    await expect(d.getByTestId("records-partition")).toHaveText("1");
    await expect(d.getByTestId("records-preview")).toContainText("This removes 0 records");
    await d.getByTestId("offset").fill("3");
    await expect(d.getByTestId("records-preview")).toContainText("This removes 3 records");
    await expect(d.getByTestId("records-submit")).toHaveText("Delete 3 records");
    await d.getByTestId("records-submit").click();
    await expect(d.getByTestId("low-watermark")).toHaveText("3");
    const after = await get(request, `/api/v1/topics/${name}/delete-records?partition=1`);
    expect(after.body.view.log_start).toBe(3);
  });

  test("on a topic that only compacts: refused POLICY_VIOLATION", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "trimc");
    await topic(request, baseURL!, name, 1, "compact");
    await produce(request, baseURL!, name, "a", "1");
    await produce(request, baseURL!, name, "b", "2");
    await page.goto(dialogUrl(name, "delete-records", "&partition=0"));
    const d = page.getByTestId("dialog-delete-records");
    await d.getByTestId("offset").fill("1");
    await d.getByTestId("records-submit").click();
    await expect(d.getByTestId("refusal-code")).toHaveText("POLICY_VIOLATION (44)");
    await expect(d.getByTestId("records-refused")).toContainText("log start is still 0");
  });
});

test.describe("Delete topic", () => {
  test("type the name to confirm; then the topic is gone and the list says so", async ({ page, request, baseURL }, info) => {
    const name = fresh(info, "gone");
    await topic(request, baseURL!, name, 2);
    await produce(request, baseURL!, name, "k", "v");
    await page.goto(`/topics/${name}`);
    await page.getByTestId("act-delete").click();
    const d = page.getByTestId("dialog-delete");
    await expect(d.getByTestId("delete-what")).toContainText("1 record");
    await expect(d.getByTestId("delete-submit")).toBeDisabled();
    await d.getByTestId("confirm").fill(name.slice(0, -1));
    await expect(d.getByTestId("delete-submit")).toBeDisabled();
    await d.getByTestId("confirm").fill(name);
    await d.getByTestId("delete-submit").click();
    await expect(page).toHaveURL(/\/topics$/);
    await expect(page.locator("[data-sonner-toast]").filter({ hasText: `Deleted ${name}.` })).toBeVisible();
    expect((await get(request, `/api/v1/topics/${name}/delete`)).status).toBe(404);
  });

  test("a topic a group reads says it is in use", async ({ page }) => {
    await page.goto(dialogUrl("orders", "delete"));
    const d = page.getByTestId("dialog-delete");
    await expect(d.getByTestId("in-use")).toContainText("fulfilment");
    await page.keyboard.press("Escape");
    await expect(d).toBeHidden();
  });
});
