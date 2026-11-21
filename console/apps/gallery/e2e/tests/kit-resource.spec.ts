// The AWS console's kit, a resource's page and what is asked on it: ResourcePage and its tabs kept in the address, the verbs,
// the result banner, tags (view, empty, edit, refused), blockers, the deletion window, the typed confirm and the section edit.
import { expect, open, overflowX, tabTo, test } from "./fixtures";

const LOOK = { theme: "aws", mode: "light" } as const;
const DARK = { theme: "aws", mode: "dark" } as const;

test("resource page: no sideways scroll at 1024 px, in either look", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  for (const look of [LOOK, DARK, { theme: "kafka", mode: "light" } as const]) {
    await open(page, "/resource", look);
    await page.waitForTimeout(400);
    expect(await overflowX(page), `${look.theme}-${look.mode}`).toBe(0);
  }
});

test("ResourcePage: header, badges, verbs (one off, and why), the sentence with its name set in type, and what it is wired to", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const q = page.getByTestId("queue-page");
  await expect(q.getByRole("heading", { level: 1 })).toHaveText("pick-lists");
  await expect(q.getByTestId("detail-badge")).toHaveText("Standard");
  await expect(q.getByTestId("verb-delete")).toBeVisible();
  await expect(q.getByTestId("verb-redrive")).toBeDisabled();
  await expect(q.getByTestId("verb-reasons")).toContainText("Start DLQ redrive is off: pick-lists-dlq holds no messages that failed.");
  await expect(q.getByTestId("detail-sentence")).toContainText("has 2 messages waiting and 1 in flight");
  await expect(q.getByTestId("detail-sentence").locator(".font-mono").first()).toHaveText("pick-lists");
  await expect(q.getByTestId("wired-link")).toHaveCount(3);
  await expect(q.getByTestId("wired-link").first()).toHaveAttribute("href", "/resources/events/rules/payment-settled");
  await q.getByTestId("verb-purge").click();
  await expect(q.getByTestId("result-banner")).toContainText("Purge was chosen.");
  // Actions: copy name, copy URL, Live.
  await q.getByTestId("actions-menu").click();
  await expect(page.getByRole("menuitem", { name: "Copy URL" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Open Live" })).toBeVisible();
  await page.keyboard.press("Escape");
});

test("ResourcePage: tabs are kept in the address, only the tab shown is drawn, and a reload keeps it", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const q = page.getByTestId("queue-page");
  await expect(q.getByTestId("tab-panel")).toContainText("No topic is subscribed to this queue");
  await expect(q.getByTestId("tab-panel").getByTestId("tags-view")).toHaveCount(0);
  const reads = () => page.evaluate(() => (window as unknown as { __mock: { calls: { path: string }[] } }).__mock.calls.filter((c) => c.path.endsWith("/pick-lists/tags")).length);
  const before = await reads();
  await q.getByTestId("tab-tagging").click();
  await expect(page).toHaveURL(/[?&]tab=tagging\b/);
  await expect(q.getByTestId("tab-panel").getByTestId("tags-view")).toBeVisible();
  expect(await reads()).toBeGreaterThan(before);
  await page.reload();
  await expect(page.getByTestId("queue-page").getByTestId("tab-tagging")).toHaveAttribute("aria-selected", "true");
  // The first tab takes no `?tab=`, and an unknown one falls back to it.
  await page.getByTestId("queue-page").getByTestId("tab-sns").click();
  await expect(page).not.toHaveURL(/tab=/);
  await page.goto("/resource?tab=gone");
  await expect(page.getByTestId("queue-page").getByTestId("tab-sns")).toHaveAttribute("aria-selected", "true");
  // Arrow keys move between tabs (Radix), and the tab list takes a ring.
  await page.getByTestId("queue-page").getByTestId("tab-sns").focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("queue-page").getByTestId("tab-lambda")).toBeFocused();
});

test("ResourcePage: a kind with nothing of its own still has a Details tab, with a long name wrapped", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const b = page.getByTestId("bare-page");
  await expect(b.getByTestId("tab-details")).toHaveAttribute("aria-selected", "true");
  await expect(b.getByTestId("field-Type")).toHaveText("SecureString");
  await expect(b.getByTestId("field-Value")).toHaveText("Hidden");
  await expect(b.getByTestId("detail-sentence")).toHaveCount(0);
  expect(await overflowX(page)).toBe(0);
});

test("ResultBanner: tones, a result that wraps, and one that goes when dismissed", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const sec = page.getByTestId("section-banner");
  await expect(sec.locator('[data-slot="result-banner"]')).toHaveCount(6);
  await expect(sec.locator('[data-tone="bad"]')).toHaveAttribute("role", "alert");
  await expect(sec.locator('[data-tone="good"]').first()).toHaveAttribute("role", "status");
  await sec.getByTestId("dismissable-dismiss").click();
  await expect(sec.getByTestId("dismissable")).toHaveCount(0);
});

test("TagsView: the tags, an edit that is checked, a refusal that stays on the form, a save that says what it did", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const sec = page.getByTestId("section-tags");
  const live = sec.getByTestId("tags-view").first();
  await expect(live.getByTestId("tags-count")).toHaveText("3 cost allocation tags");
  await expect(live.getByRole("row")).toHaveCount(4);
  await live.getByTestId("tags-edit-open").click();
  const edit = sec.getByTestId("tags-edit");
  await expect(edit.getByTestId("tags-change")).toHaveText("Changes nothing.");
  // A repeated key is refused by the form itself.
  await edit.getByTestId("tags-kv-add").click();
  await edit.getByTestId("tags-kv-key-3").fill("app");
  await expect(edit.getByText("Another row uses this key.")).toHaveCount(2);
  await edit.getByTestId("tags-save").click();
  await expect(edit.getByText("Another row uses this key.")).toHaveCount(2);
  // A reserved key is refused by the service: its words are shown and the form stays.
  await edit.getByTestId("tags-kv-key-3").fill("aws:team");
  await edit.getByTestId("tags-kv-value-3").fill("ops");
  await expect(edit.getByTestId("tags-change")).toHaveText("Adds 1 cost allocation tag.".replace("cost allocation ", ""));
  await edit.getByTestId("tags-save").click();
  await expect(edit.getByTestId("tags-refused")).toContainText("Tag keys starting with aws: are reserved");
  await edit.getByTestId("tags-kv-key-3").fill("team");
  await edit.getByTestId("tags-save").click();
  await expect(sec.getByTestId("tags-saved").first()).toContainText("The resource now has 4 cost allocation tags.");
  await expect(sec.getByTestId("tags-count").first()).toHaveText("4 cost allocation tags");
  await expect(sec.getByRole("cell", { name: "team", exact: true })).toBeVisible();
});

test("TagsView: none says what that means and offers to add; one that cannot be tagged has no edit; loading and failure say so", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const sec = page.getByTestId("section-tags");
  await expect(sec.getByTestId("empty-tab").first()).toContainText("No cost allocation tags");
  await expect(sec.getByTestId("tags-add")).toBeVisible();
  await expect(sec.getByTestId("tags-loading")).toBeVisible();
  await expect(sec.getByTestId("tags-error")).toContainText("The tags could not be read");
  // The kind that cannot be tagged: the count and the table, and no way to edit.
  await expect(sec.getByText("1 tag The console reads these but cannot change them.")).toBeVisible();
  await expect(sec.getByTestId("tags-edit-open")).toHaveCount(1);
});

test("BlockersList and DeletionWindow: clears, stops and what stays; a window that is not allowed is said; a scheduled one can be cancelled", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const sec = page.getByTestId("section-blockers");
  await expect(sec.getByTestId("blocker-Attached policies").first()).toHaveAttribute("data-tone", "warn");
  await expect(sec.getByTestId("blocker-Objects")).toHaveAttribute("data-tone", "bad");
  await expect(sec.getByTestId("blocker-Objects")).toContainText("2 objects must be removed first");
  await expect(sec.getByTestId("blocker-Objects")).toContainText("and more");
  await expect(sec.getByTestId("blocker-Layers")).toHaveAttribute("data-tone", "info");
  await expect(sec.getByTestId("blockers-kept").first()).toContainText("The policy itself stays.");
  await expect(sec.getByTestId("blockers-loading")).toBeVisible();
  const key = sec.getByTestId("deletion-window").first();
  await expect(key.getByTestId("deletion-ends")).toContainText("With 30, the key is deleted on Dec 8, 2026.");
  await key.getByTestId("deletion-days").fill("7");
  await expect(key.getByTestId("deletion-ends")).toContainText("With 7, the key is deleted on Nov 15, 2026.");
  await key.getByTestId("deletion-days").fill("45");
  await expect(key.getByTestId("deletion-ends")).toContainText("Enter a whole number of days from 7 to 30.");
  await expect(key.getByTestId("deletion-days")).toHaveAttribute("aria-invalid", "true");
  await expect(sec.getByTestId("deletion-scheduled")).toContainText("scheduled for deletion on 2026-11-15 09:00:00 UTC");
  await expect(sec.getByTestId("deletion-restore")).toHaveText("Cancel deletion");
});

test("TypedConfirm: off until the name is typed, Esc and Cancel close it to its opener, a refusal shows in place", async ({ page, browserName }) => {
  await open(page, "/resource", LOOK);
  const opener = page.getByTestId("open-role-delete");
  await opener.click();
  const dialog = page.getByTestId("role-confirm");
  await expect(dialog).toBeVisible();
  await expect(page.locator("#root")).toHaveAttribute("inert", "");
  await expect(dialog.getByTestId("blocker-Attached policies")).toContainText("1 attached policy is cleared with it");
  const submit = dialog.getByTestId("role-confirm-submit");
  await expect(submit).toBeDisabled();
  await dialog.getByTestId("role-confirm-input").fill("larder-admi");
  await expect(submit).toBeDisabled();
  await dialog.getByTestId("role-confirm-input").fill("Larder-admin");
  await expect(submit).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(opener).toBeFocused();
  await opener.click();
  // What was typed before is not kept.
  await expect(dialog.getByTestId("role-confirm-input")).toHaveValue("");
  await dialog.getByTestId("role-confirm-input").fill("larder-admin ");
  await expect(submit).toBeEnabled();
  await dialog.getByTestId("role-confirm-input").press("Enter");
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("role-deleted")).toBeVisible();
  const b = page.getByTestId("open-bucket-delete");
  await b.click();
  await expect(page.getByTestId("bucket-refused")).toContainText("BucketNotEmpty");
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(b).toBeFocused();
  void browserName;
});

test("TypedConfirm for a key: the waiting period is part of the dialog", async ({ page }) => {
  await open(page, "/resource", LOOK);
  await page.getByTestId("open-key-delete").click();
  const d = page.getByTestId("key-confirm");
  await expect(d.getByTestId("deletion-window")).toBeVisible();
  await d.getByTestId("key-confirm-input").fill("alias/larder-app");
  await expect(d.getByTestId("key-confirm-submit")).toBeEnabled();
});

test("SectionEdit: fields and the call they make, off with a reason, the service's refusal in place, focus back to the opener", async ({ page }) => {
  await open(page, "/resource", LOOK);
  const opener = page.getByTestId("open-retention");
  await opener.click();
  const d = page.getByTestId("retention-edit");
  await expect(d.getByTestId("retention-edit-call-code")).toContainText('"RetentionPeriodHours": 48');
  await d.getByTestId("retention-hours").fill("5");
  await expect(d.getByTestId("retention-edit-save")).toBeDisabled();
  await expect(d).toContainText("Retention is 24 to 8,760 hours.");
  await expect(d.getByTestId("retention-edit-call-missing")).toHaveText("Needs RetentionPeriodHours.");
  await page.keyboard.press("Escape");
  await expect(d).toBeHidden();
  await expect(opener).toBeFocused();
  await page.getByTestId("open-cors").click();
  const c = page.getByTestId("cors-edit");
  await expect(c.getByTestId("cors-edit-call-code")).toContainText("PutBucketCors");
  await expect(c.getByTestId("cors-edit-refused")).toContainText("The CORS configuration could not be saved.");
  await expect(c.getByTestId("cors-edit-call-came-back")).toBeVisible();
});

test("keyboard: a verb and a tab take the focus with a ring", async ({ page, browserName }) => {
  await open(page, "/resource", LOOK);
  const verb = page.getByTestId("queue-page").getByTestId("verb-edit");
  await tabTo(page, browserName, verb, 200);
  await expect(verb).toBeFocused();
});
