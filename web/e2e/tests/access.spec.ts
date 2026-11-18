// Access (slice R4): the switches, users, the ACL table with its filters, Create ACL and Delete ACL (what the broker decides
// afterwards), Kafka's refusal.
import { choose, expect, test } from "./fixtures";
import { apiGet, noMemory, noNativeSelect, widths } from "./r4";

test("Access: the three switches, users, ACLs, the console card and the start warnings; no secret, no native control", async ({ page }) => {
  await page.goto("/access");
  await expect(page.locator("h1")).toHaveText("Access");
  for (const id of ["auth-card", "authz-card", "tls-card", "console-card", "warnings-card"]) await expect(page.getByTestId(id)).toBeVisible();
  await expect(page.getByTestId("console-card")).toContainText("User:__console");
  await expect(page.getByTestId("warnings-card").locator("li")).toHaveCount(3);
  await expect(page.getByRole("table", { name: "Users" })).toBeVisible();
  await expect(page.getByRole("table", { name: "ACLs" })).toBeVisible();
  const text = (await page.locator("body").innerText()).toLowerCase();
  for (const word of ["password=", "salt:", "stored_key", "server_key"]) expect(text).not.toContain(word);
  await noMemory(page);
  await noNativeSelect(page);
  await widths(page);
});

test("Access: Create ACL by mouse and keyboard, it shows in the table, the filters find it, Delete removes it", async ({ page, browserName }) => {
  const who = `User:e2e-${browserName}`;
  await page.goto("/access");
  // CreateAcls is idempotent, so a leftover of an earlier run is the same one row.
  await page.getByTestId("acl-create").click();
  await expect(page).toHaveURL(/acl=new/);
  const dialog = page.getByTestId("acl-create-dialog");
  await expect(dialog.getByTestId("acl-principal")).toBeFocused();
  await page.keyboard.type(who);
  await dialog.getByTestId("acl-host").fill("*");
  // The resource type Select: away and back, by mouse.
  await choose(page, dialog.getByTestId("acl-type"), "Group");
  await expect(dialog.getByTestId("call-box")).toContainText("GROUP");
  await choose(page, dialog.getByTestId("acl-type"), "Topic");
  await dialog.getByTestId("acl-pattern").getByRole("radio", { name: "Prefixed" }).click();
  await dialog.getByTestId("acl-name").fill("payments.");
  await choose(page, dialog.getByTestId("acl-op"), "Write");
  await dialog.getByTestId("acl-perm-choice").getByRole("radio", { name: "Deny" }).click();
  await expect(dialog.getByTestId("call-box")).toContainText("PREFIXED");
  await expect(dialog.getByTestId("call-box")).toContainText(who);
  await noNativeSelect(page);
  await dialog.getByTestId("acl-create-submit").click();
  // One pattern for every write (CON-1354): the result in place, then Done.
  await expect(dialog.getByTestId("acl-created")).toContainText(who);
  await dialog.getByTestId("write-done").click();
  await expect(dialog).toBeHidden();
  await expect(page).not.toHaveURL(/acl=new/);
  const row = page.getByRole("table", { name: "ACLs" }).locator("tbody tr", { hasText: who });
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Prefixed");
  await expect(row).toContainText("Write");
  await expect(row).toContainText("Deny");
  // The broker has it.
  const api = await apiGet(page.request, "/api/v1/access");
  expect(api.access.acls.rows.some((e: { principal: string }) => e.principal === who)).toBe(true);

  // Filters are in the address.
  await page.getByTestId("acl-q").fill(`e2e-${browserName}`);
  await expect(page).toHaveURL(new RegExp(`q=e2e-${browserName}`));
  await choose(page, page.getByTestId("acl-kind"), "Group");
  await expect(page).toHaveURL(/kind=group/);
  await expect(page.getByRole("table", { name: "ACLs" }).locator("tbody tr")).toHaveCount(0);
  await choose(page, page.getByTestId("acl-kind"), "Topic");
  await page.getByTestId("acl-perm").getByRole("radio", { name: "Deny" }).click();
  await expect(page).toHaveURL(/perm=deny/);
  await expect(row).toHaveCount(1);

  // Delete: what the broker decides afterwards, then gone from the table and the broker.
  await row.getByTestId("acl-delete").click();
  await expect(page).toHaveURL(/delete=/);
  const del = page.getByTestId("acl-delete-dialog");
  await expect(del.getByTestId("acl-delete-entry")).toContainText(who);
  await expect(del.getByTestId("acl-after")).toContainText(/./);
  await expect(del.getByTestId("call-box")).toContainText("DeleteAcls");
  await del.getByTestId("acl-delete-confirm").click();
  await expect(del.getByTestId("acl-deleted")).toBeVisible();
  await del.getByTestId("write-done").click();
  await expect(del).toBeHidden();
  await expect(row).toHaveCount(0);
  const after = await apiGet(page.request, "/api/v1/access");
  expect(after.access.acls.rows.some((e: { principal: string }) => e.principal === who)).toBe(false);
});

test("Access: a missing field is said under it; Kafka's refusal is in Kafka's words; Esc and Back close the dialog", async ({ page }) => {
  await page.goto("/access");
  const open = page.getByTestId("acl-create");
  await open.click();
  const dialog = page.getByTestId("acl-create-dialog");
  await dialog.getByTestId("acl-create-submit").click();
  await expect(dialog).toContainText("Type the principal, as Type:name such as User:orders.");
  await expect(dialog).toContainText("Type the resource's name.");
  // A principal Kafka does not take (no Type: in front): the broker refuses, with its code and words.
  await dialog.getByTestId("acl-principal").fill("nobody-without-a-type");
  await dialog.getByTestId("acl-name").fill("orders");
  await dialog.getByTestId("acl-create-submit").click();
  await expect(dialog.getByTestId("refusal-name")).toHaveText(/^[A-Z_]+$/);
  await expect(dialog.getByTestId("refusal-message")).not.toBeEmpty();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(open).toBeFocused();
  await open.click();
  await expect(dialog).toBeVisible();
  await page.goBack();
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(/\/access$/);
});
