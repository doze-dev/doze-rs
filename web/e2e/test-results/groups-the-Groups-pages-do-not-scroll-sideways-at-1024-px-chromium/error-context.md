# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: groups.spec.ts >> the Groups pages do not scroll sideways at 1024 px
- Location: tests/groups.spec.ts:323:3

# Error details

```
Test timeout of 45000ms exceeded.
```

```
Error: goto: Test timeout of 45000ms exceeded.
Call log:
  - navigating to "http://127.0.0.1:39094/groups/r2-busy-chromium", waiting until "load"

```

# Test source

```ts
  226 |   await picker.getByRole("button", { name: /^1 / }).click();
  227 |   await picker.getByTestId("datetime-clock").fill("00:00:00");
  228 |   await picker.getByTestId("datetime-done").click();
  229 |   await expect(dialog.getByTestId("datetime-trigger")).toContainText(/\d{4}-\d{2}-01 00:00:00 UTC/);
  230 |   await dialog.getByTestId("reset-preview-button").click();
  231 |   await expect(dialog.getByTestId("reset-error")).toHaveCount(0);
  232 |   // Earliest: every partition goes to its log start.
  233 |   await dialog.getByRole("radio", { name: /Earliest/ }).click();
  234 |   await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  235 |   await dialog.getByTestId("reset-preview-button").click();
  236 |   await expect(dialog.getByTestId("reset-after")).toHaveText(["0", "0", "0"]);
  237 |   await expect(dialog.getByTestId("reset-submit")).toHaveText(/Reset 3 offsets/);
  238 |   await dialog.getByTestId("reset-submit").click();
  239 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset 3 offsets" })).toBeVisible();
  240 |   await expect(dialog).toBeHidden();
  241 |   await expect(page).not.toHaveURL(/dialog=/);
  242 |   const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  243 |   await expect(block.locator("tbody tr td:nth-child(3)")).toHaveText(["0", "0", "0"]);
  244 |   await expect(block.getByTestId("topic-behind")).not.toHaveText("0 behind");
  245 |   // Back to the end, with the keyboard: Esc cancels, Enter in the dialog previews.
  246 |   await page.getByTestId("open-reset").click();
  247 |   await expect(dialog).toBeVisible();
  248 |   await page.keyboard.press("Escape");
  249 |   await expect(dialog).toBeHidden();
  250 |   await page.getByTestId("open-reset").click();
  251 |   await dialog.getByRole("radio", { name: /Latest/ }).click();
  252 |   await dialog.getByTestId("reset-preview-button").click();
  253 |   await dialog.getByTestId("reset-submit").click();
  254 |   await expect(block.getByTestId("topic-behind")).toHaveText("0 behind");
  255 | });
  256 | 
  257 | test("Reset offsets of a group with a member is refused, in Kafka's name and words, and nothing changes", async ({ page }) => {
  258 |   await page.goto(`/groups/${member}?dialog=reset`);
  259 |   const dialog = page.getByTestId("dialog-reset");
  260 |   await expect(dialog.getByTestId("reset-not-empty")).toContainText("This group is not empty.");
  261 |   await dialog.getByRole("radio", { name: /Earliest/ }).click();
  262 |   await dialog.getByTestId("reset-preview-button").click();
  263 |   await expect(dialog.getByTestId("reset-after").first()).toHaveText("0");
  264 |   await dialog.getByTestId("reset-submit").click();
  265 |   const refusal = dialog.getByTestId("refusal");
  266 |   await expect(refusal).toBeVisible();
  267 |   await expect(refusal.getByTestId("refusal-code")).toHaveText(/^[A-Z_]+ \(\d+\)$/);
  268 |   await expect(refusal.getByTestId("refusal-words")).toHaveText(/^".+"$/);
  269 |   await expect(dialog).toContainText("What would pass");
  270 |   await expect(dialog).toContainText("The call that was refused");
  271 |   await dialog.getByTestId("dialog-cancel").click();
  272 |   await expect(dialog).toBeHidden();
  273 |   await page.getByTestId("group-tabs").getByRole("link", { name: /^Lag/ }).click();
  274 |   await expect(page.getByTestId("lag-topic").first().getByTestId("topic-behind")).toHaveText("0 behind");
  275 | });
  276 | 
  277 | test("Delete group with a member is refused as NON_EMPTY_GROUP in Kafka's words", async ({ page }) => {
  278 |   await page.goto(`/groups/${member}`);
  279 |   await page.getByTestId("open-delete").click();
  280 |   const dialog = page.getByTestId("dialog-delete");
  281 |   await expect(dialog).toContainText(`Delete ${member}?`);
  282 |   await expect(dialog.getByTestId("delete-not-empty")).toContainText("NON_EMPTY_GROUP");
  283 |   await dialog.getByTestId("delete-submit").click();
  284 |   await expect(dialog.getByTestId("refusal-code")).toHaveText("NON_EMPTY_GROUP (68)");
  285 |   await expect(dialog.getByTestId("refusal-words")).toHaveText('"The group is not empty."');
  286 |   await expect(dialog).toContainText("Every member has to leave");
  287 |   await page.keyboard.press("Escape");
  288 |   await expect(dialog).toBeHidden();
  289 |   await expect(page.getByTestId("dialog-cancel")).toHaveCount(0);
  290 |   await expect(page.getByTestId("open-delete")).toBeFocused();
  291 |   await expect(page.locator("h1")).toHaveText(member);
  292 | });
  293 | 
  294 | test("Delete offsets forgets the commits of the topics ticked; Delete group then removes the empty group", async ({ page }) => {
  295 |   await page.goto(`/groups/${idle}/lag`);
  296 |   await page.getByTestId("open-delete-offsets").click();
  297 |   const dialog = page.getByTestId("dialog-delete-offsets");
  298 |   const box = dialog.getByTestId("offsets-topic");
  299 |   await expect(box).toHaveCount(1);
  300 |   await expect(box).toBeChecked();
  301 |   await box.click();
  302 |   await expect(dialog.getByTestId("offsets-submit")).toBeDisabled();
  303 |   await dialog.getByText(TOPIC, { exact: true }).click();
  304 |   await expect(box).toBeChecked();
  305 |   await expect(dialog.getByTestId("offsets-submit")).toHaveText(/Delete 3 offsets/);
  306 |   await dialog.getByTestId("offsets-submit").click();
  307 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Deleted 3 committed offsets" })).toBeVisible();
  308 |   await expect(dialog).toBeHidden();
  309 |   await page.getByTestId("open-delete").click();
  310 |   const del = page.getByTestId("dialog-delete");
  311 |   await expect(del).toContainText(`Delete ${idle}?`);
  312 |   await del.getByTestId("delete-submit").click();
  313 |   await expect(page).toHaveURL(/\/groups$/);
  314 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: `Deleted ${idle}.` })).toBeVisible();
  315 |   await expect(page.getByTestId("group-link").filter({ hasText: idle })).toHaveCount(0);
  316 |   // The API now says there is no such group (a page that asked would log the 404 in Safari).
  317 |   const gone = await page.request.get(`/api/v1/groups/${idle}`, { headers: { "X-Console": "1" } });
  318 |   expect(gone.status()).toBe(404);
  319 |   expect((await gone.json()).code).toBe("GROUP_NOT_FOUND");
  320 | });
  321 | 
  322 | for (const width of [1024, 1280, 1440, 1920]) {
  323 |   test(`the Groups pages do not scroll sideways at ${width} px`, async ({ page }) => {
  324 |     await page.setViewportSize({ width, height: 900 });
  325 |     for (const path of ["/groups", `/groups/${member}`, `/groups/${member}/lag`, `/groups/${member}/lag?expiring=1`, `/groups/${member}/story`, `/groups/${member}/wire`, `/groups/${member}?dialog=reset`, `/groups/${member}/story?rule=1`]) {
> 326 |       await page.goto(path);
      |                  ^ Error: goto: Test timeout of 45000ms exceeded.
  327 |       await expect(page.locator("h1")).toBeVisible();
  328 |       await page.waitForLoadState("networkidle");
  329 |       expect(await overflowX(page), `${path} at ${width}`).toBeLessThanOrEqual(0);
  330 |     }
  331 |   });
  332 | }
  333 | 
```