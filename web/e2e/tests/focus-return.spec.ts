// Every overlay gives the focus back to what opened it (CON-1290): opened by the keyboard and closed with Esc, and opened by
// the mouse and closed with its close button, in Firefox, WebKit and Chromium. One shared mechanism (web/src/lib/opener.ts)
// does it for every Dialog and Sheet; this test walks all of them, so an overlay added without it fails here.
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { apiGet, apiPost } from "./r4";

type Case = {
  name: string;
  url: string;
  opener: (p: Page) => Locator;
  overlay: string;
  /** Opens by the keyboard from the focused opener (default: Enter). */
  keys?: (p: Page) => Promise<void>;
  /** Opens by the mouse (default: a click on the opener). */
  click?: (p: Page) => Promise<void>;
  /** Makes what the overlay needs (a rule, an ACL). */
  before?: (p: Page, baseURL: string) => Promise<void>;
};

async function labRule(p: Page, baseURL: string) {
  const home = await apiGet(p.request, "/api/v1/lab");
  if (home.home.active.length > 0) return;
  const form = await apiGet(p.request, "/api/v1/lab/form?kind=slow");
  const made = await apiPost(p.request, baseURL, "/api/v1/lab/rules", { kind: "slow", fields: { ...form.fields, client: "focus-return-nobody" }, check: false });
  expect(made.status).toBe(200);
}

async function acl(p: Page, baseURL: string) {
  const a = await apiGet(p.request, "/api/v1/access");
  if (a.access.acls.rows.some((r: { principal: string }) => r.principal === "User:focus-return")) return;
  const made = await apiPost(p.request, baseURL, "/api/v1/access/acls/new", { principal: "User:focus-return", host: "*", type: "topic", pattern: "literal", name: "orders", op: "read", perm: "allow" });
  expect(made.status).toBe(200);
}

const menu = (name: string) => ({
  keys: async (p: Page) => {
    await p.keyboard.press("Enter");
    const item = p.getByRole("menuitem", { name });
    await expect(item).toBeVisible();
    await item.focus();
    await p.keyboard.press("Enter");
  },
  click: async (p: Page) => {
    await p.getByRole("button", { name: "Actions for cleanup.policy" }).click();
    await p.getByRole("menuitem", { name }).click();
  },
});

const CASES: Case[] = [
  { name: "Produce", url: "/topics/orders", opener: (p) => p.getByTestId("act-produce"), overlay: "dialog-produce" },
  { name: "Delete topic", url: "/topics/orders", opener: (p) => p.getByTestId("act-delete"), overlay: "dialog-delete" },
  { name: "Add partitions", url: "/topics/orders/partitions", opener: (p) => p.getByTestId("act-add-partitions"), overlay: "dialog-add-partitions" },
  { name: "Delete records", url: "/topics/orders/partitions", opener: (p) => p.getByTestId("delete-records-0"), overlay: "dialog-delete-records" },
  { name: "Config: Edit", url: "/topics/inventory.levels/config", opener: (p) => p.getByRole("button", { name: "Actions for cleanup.policy" }), overlay: "dialog-config-edit", ...menu("Edit") },
  { name: "Config: Reset to default", url: "/topics/inventory.levels/config", opener: (p) => p.getByRole("button", { name: "Actions for cleanup.policy" }), overlay: "dialog-config-reset", ...menu("Reset to the inherited value") },
  { name: "Record sheet", url: "/topics/orders/messages", opener: (p) => p.getByTestId("open-record").first(), overlay: "record-sheet" },
  { name: "Live: request sheet", url: "/live", opener: (p) => p.getByTestId("row-time").first(), overlay: "request-sheet" },
  { name: "Live: clients sheet", url: "/live", opener: (p) => p.getByTestId("open-clients"), overlay: "clients-sheet" },
  { name: "Group: Reset offsets", url: "/groups/fulfilment", opener: (p) => p.getByTestId("open-reset"), overlay: "dialog-reset" },
  { name: "Group: Delete offsets", url: "/groups/fulfilment", opener: (p) => p.getByTestId("open-delete-offsets"), overlay: "dialog-delete-offsets" },
  { name: "Lab: New rehearsal", url: "/lab", opener: (p) => p.getByTestId("lab-new").first(), overlay: "lab-dialog" },
  { name: "Lab: rule sheet", url: "/lab", opener: (p) => p.getByTestId("rule-open").first(), overlay: "lab-rule-sheet", before: labRule },
  { name: "Lab: Remove rule", url: "/lab", opener: (p) => p.getByTestId("rule-remove").first(), overlay: "lab-remove-dialog", before: labRule },
  { name: "Access: Create ACL", url: "/access", opener: (p) => p.getByTestId("acl-create"), overlay: "acl-create-dialog" },
  { name: "Access: Delete ACL", url: "/access", opener: (p) => p.getByTestId("acl-delete").first(), overlay: "acl-delete-dialog", before: acl },
  { name: "Settings: Edit", url: "/settings", opener: (p) => p.getByTestId("edit-log.segment.bytes"), overlay: "settings-edit" },
];

async function settled(p: Page, overlay: string) {
  await expect(p.getByTestId(overlay)).toBeVisible();
  // At a person's pace: the opening animation and the first focus inside settle.
  await p.waitForTimeout(250);
}

for (const c of CASES) {
  test(`Focus returns to the opener: ${c.name} (keyboard + Esc, mouse + close button)`, async ({ page, baseURL }) => {
    await c.before?.(page, baseURL!);
    await page.goto(c.url);
    const opener = c.opener(page);
    await expect(opener).toBeVisible();

    await opener.focus();
    if (c.keys) await c.keys(page);
    else await page.keyboard.press("Enter");
    await settled(page, c.overlay);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId(c.overlay)).toHaveCount(0);
    await expect(opener).toBeFocused();

    // The mouse: Firefox and Safari do not focus a clicked button, so this is the case Radix alone loses.
    await page.mouse.move(0, 0);
    await page.locator("body").evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    if (c.click) await c.click(page);
    else await opener.click();
    await settled(page, c.overlay);
    await page.getByTestId(c.overlay).getByRole("button", { name: "Close", exact: true }).last().click();
    await expect(page.getByTestId(c.overlay)).toHaveCount(0);
    await expect(opener).toBeFocused();
  });
}

test("Focus returns to the opener after a refresh drew it again while the overlay was open", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  const opener = page.getByTestId("open-record").first();
  const row = await opener.evaluate((el) => el.closest("tr")?.dataset.row);
  await opener.click();
  await settled(page, "record-sheet");
  // The table draws its rows again (a refresh): the opener's node is replaced by a new one for the same record.
  await page.evaluate((r) => {
    const btn = document.querySelector<HTMLElement>(`tr[data-row="${r}"] [data-testid=open-record]`)!;
    btn.replaceWith(btn.cloneNode(true));
  }, row);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("record-sheet")).toHaveCount(0);
  await expect(page.locator(`tr[data-row="${row}"] [data-testid=open-record]`)).toBeFocused();
});
