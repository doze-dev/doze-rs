# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: groups.spec.ts >> Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads
- Location: tests/groups.spec.ts:211:1

# Error details

```
Error: expect(locator).not.toContainText(expected) failed

Locator: getByTestId('lag-topic').filter({ has: getByRole('link', { name: 'r2-orders' }) })
Expected substring: not "0 behind"
Received string: "r2-orders3 partitions, no member holds any30 behindPartitionLog endCommittedLagLast commitr2-orders-080800:22:51r2-orders-11201200:22:51r2-orders-21001000:22:51"
Timeout: 8000ms

Call log:
  - Expect "not toContainText" getByTestId('lag-topic').filter({ has: getByRole('link', { name: 'r2-orders' }) }) with timeout 8000ms
  - waiting for getByTestId('lag-topic').filter({ has: getByRole('link', { name: 'r2-orders' }) })
    19 × locator resolved to <section data-topic="r2-orders" data-testid="lag-topic" class="flex flex-col gap-2">…</section>
       - unexpected value "r2-orders3 partitions, no member holds any30 behindPartitionLog endCommittedLagLast commitr2-orders-080800:22:51r2-orders-11201200:22:51r2-orders-21001000:22:51"

```

```yaml
- link "r2-orders":
  - /url: /topics/r2-orders
- text: 3 partitions, no member holds any 30 behind
- table "Lag of r2-orders":
  - rowgroup:
    - row "Partition Log end Committed Lag Last commit":
      - columnheader "Partition":
        - button "Partition"
      - columnheader "Log end":
        - button "Log end"
      - columnheader "Committed":
        - button "Committed"
      - columnheader "Lag":
        - button "Lag"
      - columnheader "Last commit":
        - button "Last commit"
  - rowgroup:
    - row "r2-orders-0 8 0 8 00:22:51":
      - cell "r2-orders-0"
      - cell "8"
      - cell "0"
      - cell "8"
      - cell "00:22:51"
    - row "r2-orders-1 12 0 12 00:22:51":
      - cell "r2-orders-1"
      - cell "12"
      - cell "0"
      - cell "12"
      - cell "00:22:51"
    - row "r2-orders-2 10 0 10 00:22:51":
      - cell "r2-orders-2"
      - cell "10"
      - cell "0"
      - cell "10"
      - cell "00:22:51"
```

# Test source

```ts
  146 |   await expect(page).toHaveURL(new RegExp(`/groups/${member}$`));
  147 | });
  148 | 
  149 | test("Lag: one table per topic with the lag as a bar, and when the commits expire as a choice in the address", async ({ page }) => {
  150 |   await page.goto(`/groups/${idle}/lag`);
  151 |   const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  152 |   await expect(block).toBeVisible();
  153 |   await expect(block.locator("tbody tr")).toHaveCount(3);
  154 |   await expect(block).toContainText("0 behind");
  155 |   await expect(page.getByTestId("lag-explain")).toBeVisible();
  156 |   await page.getByTestId("lag-view").getByRole("radio", { name: "When do these expire?" }).click();
  157 |   await expect(page).toHaveURL(/expiring=1/);
  158 |   await expect(page.getByText("Retention window")).toBeVisible();
  159 |   await expect(block.getByRole("columnheader", { name: "Forgotten at" })).toBeVisible();
  160 |   await expect(page.getByTestId("lag-explain")).toHaveAttribute("data-state", "expiring");
  161 |   await page.goBack();
  162 |   await expect(page).not.toHaveURL(/expiring=1/);
  163 |   await expect(block.getByRole("columnheader", { name: "Lag" })).toBeVisible();
  164 | });
  165 | 
  166 | test("Story: the generations newest first; a rebalance opens as a Sheet, the flapping rule as another", async ({ page }) => {
  167 |   await page.goto(`/groups/${member}/story`);
  168 |   const gens = page.getByTestId("generation");
  169 |   await expect(gens.first()).toContainText("Member joined");
  170 |   await expect(page.locator("[data-slot=stat-strip]").nth(1)).toContainText("Rebalances kept");
  171 |   const open = page.getByTestId("open-rebalance").first();
  172 |   await open.click();
  173 |   const sheet = page.getByTestId("rebalance-sheet");
  174 |   await expect(sheet).toBeVisible();
  175 |   await expect(page).toHaveURL(/generation=\d+/);
  176 |   await expect(sheet).toContainText("Began");
  177 |   await expect(sheet).toContainText("Members");
  178 |   await page.keyboard.press("Escape");
  179 |   await expect(sheet).toBeHidden();
  180 |   await expect(page).not.toHaveURL(/generation=/);
  181 |   await expect(open).toBeFocused();
  182 |   // A quiet group's page says so; one with more than one rebalance has the flapping card.
  183 |   const rule = page.getByTestId("check-rule");
  184 |   if (await rule.count()) {
  185 |     await rule.click();
  186 |     await expect(page.getByTestId("rule-sheet")).toBeVisible();
  187 |     await expect(page).toHaveURL(/rule=1/);
  188 |     await page.goBack();
  189 |     await expect(page.getByTestId("rule-sheet")).toBeHidden();
  190 |   } else await expect(page.getByTestId("story-quiet")).toContainText("A quiet story is a good one");
  191 | });
  192 | 
  193 | test("Wire: the requests that touched the group, filtered by kind and by text in the address", async ({ page }) => {
  194 |   await page.goto(`/groups/${member}/wire`);
  195 |   const table = page.getByRole("table", { name: "Requests that touched this group" });
  196 |   await expect(table.locator("tbody tr").first()).toBeVisible();
  197 |   const chips = page.getByTestId("wire-chips");
  198 |   await chips.getByRole("radio", { name: /^Commits/ }).click();
  199 |   await expect(page).toHaveURL(/kind=commits/);
  200 |   await expect(chips.getByRole("radio", { name: /^Commits/ })).toHaveAttribute("aria-checked", "true");
  201 |   for (const t of await table.locator("tbody tr td:nth-child(3)").allInnerTexts()) expect(t).toMatch(/Offset/);
  202 |   await chips.getByRole("radio", { name: /^Join and sync/ }).click();
  203 |   await expect(page).toHaveURL(/kind=join/);
  204 |   await page.getByTestId("wire-filter").fill(`${member}-app`);
  205 |   await expect(page).toHaveURL(/q=/);
  206 |   await chips.getByRole("radio", { name: /^All/ }).click();
  207 |   await expect(page).not.toHaveURL(/kind=/);
  208 |   for (const t of await table.locator("tbody tr td:nth-child(2)").allInnerTexts()) expect(t).toContain(`${member}-app`);
  209 | });
  210 | 
  211 | test("Reset offsets: a GET preview, then the reset; the Lag tab shows where the group now reads", async ({ page }) => {
  212 |   await page.goto(`/groups/${idle}/lag`);
  213 |   await page.getByTestId("open-reset").click();
  214 |   const dialog = page.getByTestId("dialog-reset");
  215 |   await expect(dialog).toBeVisible();
  216 |   await expect(page).toHaveURL(/dialog=reset/);
  217 |   await expect(dialog).toContainText("has no members");
  218 |   // A value that is not a number: the preview says so in the console's words, and Reset is off.
  219 |   await dialog.getByRole("radio", { name: /A specific offset/ }).click();
  220 |   await dialog.getByTestId("reset-value").fill("abc");
  221 |   await dialog.getByTestId("reset-preview-button").click();
  222 |   await expect(dialog.getByTestId("reset-error")).toHaveText("The offset must be a whole number.");
  223 |   await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  224 |   // A time, chosen with the picker (never the browser's own widget): a day of the grid and a time typed.
  225 |   await dialog.getByRole("radio", { name: /A time/ }).click();
  226 |   await dialog.getByTestId("datetime-trigger").click();
  227 |   const picker = page.getByTestId("datetime-popover");
  228 |   await picker.getByRole("button", { name: /^1 / }).click();
  229 |   await picker.getByTestId("datetime-clock").fill("00:00:00");
  230 |   await picker.getByTestId("datetime-done").click();
  231 |   await expect(dialog.getByTestId("datetime-trigger")).toContainText(/\d{4}-\d{2}-01 00:00:00 UTC/);
  232 |   await dialog.getByTestId("reset-preview-button").click();
  233 |   await expect(dialog.getByTestId("reset-error")).toHaveCount(0);
  234 |   // Earliest: every partition goes to its log start.
  235 |   await dialog.getByRole("radio", { name: /Earliest/ }).click();
  236 |   await expect(dialog.getByTestId("reset-submit")).toBeDisabled();
  237 |   await dialog.getByTestId("reset-preview-button").click();
  238 |   await expect(dialog.getByTestId("reset-after")).toHaveText(["0", "0", "0"]);
  239 |   await expect(dialog.getByTestId("reset-submit")).toHaveText(/Reset 3 offsets/);
  240 |   await dialog.getByTestId("reset-submit").click();
  241 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset 3 offsets" })).toBeVisible();
  242 |   await expect(dialog).toBeHidden();
  243 |   await expect(page).not.toHaveURL(/dialog=/);
  244 |   const block = page.getByTestId("lag-topic").filter({ has: page.getByRole("link", { name: TOPIC }) });
  245 |   await expect(block.locator("tbody tr td:nth-child(3)")).toHaveText(["0", "0", "0"]);
> 246 |   await expect(block).not.toContainText("0 behind");
      |                           ^ Error: expect(locator).not.toContainText(expected) failed
  247 |   // Back to the end, with the keyboard: Esc cancels, Enter in the dialog previews.
  248 |   await page.getByTestId("open-reset").click();
  249 |   await expect(dialog).toBeVisible();
  250 |   await page.keyboard.press("Escape");
  251 |   await expect(dialog).toBeHidden();
  252 |   await page.getByTestId("open-reset").click();
  253 |   await dialog.getByRole("radio", { name: /Latest/ }).click();
  254 |   await dialog.getByTestId("reset-preview-button").click();
  255 |   await dialog.getByTestId("reset-submit").click();
  256 |   await expect(block).toContainText("0 behind");
  257 | });
  258 | 
  259 | test("Reset offsets of a group with a member is refused, in Kafka's name and words, and nothing changes", async ({ page }) => {
  260 |   await page.goto(`/groups/${member}?dialog=reset`);
  261 |   const dialog = page.getByTestId("dialog-reset");
  262 |   await expect(dialog.getByTestId("reset-not-empty")).toContainText("This group is not empty.");
  263 |   await dialog.getByRole("radio", { name: /Earliest/ }).click();
  264 |   await dialog.getByTestId("reset-preview-button").click();
  265 |   await expect(dialog.getByTestId("reset-after").first()).toHaveText("0");
  266 |   await dialog.getByTestId("reset-submit").click();
  267 |   const refusal = dialog.getByTestId("refusal");
  268 |   await expect(refusal).toBeVisible();
  269 |   await expect(refusal.getByTestId("refusal-code")).toHaveText(/^[A-Z_]+ \(\d+\)$/);
  270 |   await expect(refusal.getByTestId("refusal-words")).toHaveText(/^".+"$/);
  271 |   await expect(dialog).toContainText("What would pass");
  272 |   await expect(dialog).toContainText("The call that was refused");
  273 |   await dialog.getByTestId("dialog-cancel").click();
  274 |   await expect(dialog).toBeHidden();
  275 |   await page.getByTestId("group-tabs").getByRole("link", { name: /^Lag/ }).click();
  276 |   await expect(page.getByTestId("lag-topic").first()).toContainText("0 behind");
  277 | });
  278 | 
  279 | test("Delete group with a member is refused as NON_EMPTY_GROUP in Kafka's words", async ({ page }) => {
  280 |   await page.goto(`/groups/${member}`);
  281 |   await page.getByTestId("open-delete").click();
  282 |   const dialog = page.getByTestId("dialog-delete");
  283 |   await expect(dialog).toContainText(`Delete ${member}?`);
  284 |   await expect(dialog.getByTestId("delete-not-empty")).toContainText("NON_EMPTY_GROUP");
  285 |   await dialog.getByTestId("delete-submit").click();
  286 |   await expect(dialog.getByTestId("refusal-code")).toHaveText("NON_EMPTY_GROUP (68)");
  287 |   await expect(dialog.getByTestId("refusal-words")).toHaveText('"The group is not empty."');
  288 |   await expect(dialog).toContainText("Every member has to leave");
  289 |   await page.keyboard.press("Escape");
  290 |   await expect(dialog).toBeHidden();
  291 |   await expect(page.getByTestId("dialog-cancel")).toHaveCount(0);
  292 |   await expect(page.getByTestId("open-delete")).toBeFocused();
  293 |   await expect(page.locator("h1")).toHaveText(member);
  294 | });
  295 | 
  296 | test("Delete offsets forgets the commits of the topics ticked; Delete group then removes the empty group", async ({ page }) => {
  297 |   await page.goto(`/groups/${idle}/lag`);
  298 |   await page.getByTestId("open-delete-offsets").click();
  299 |   const dialog = page.getByTestId("dialog-delete-offsets");
  300 |   const box = dialog.getByTestId("offsets-topic");
  301 |   await expect(box).toHaveCount(1);
  302 |   await expect(box).toBeChecked();
  303 |   await box.click();
  304 |   await expect(dialog.getByTestId("offsets-submit")).toBeDisabled();
  305 |   await dialog.getByText(TOPIC, { exact: true }).click();
  306 |   await expect(box).toBeChecked();
  307 |   await expect(dialog.getByTestId("offsets-submit")).toHaveText(/Delete 3 offsets/);
  308 |   await dialog.getByTestId("offsets-submit").click();
  309 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Deleted 3 committed offsets" })).toBeVisible();
  310 |   await expect(dialog).toBeHidden();
  311 |   await page.getByTestId("open-delete").click();
  312 |   const del = page.getByTestId("dialog-delete");
  313 |   await expect(del).toContainText(`Delete ${idle}?`);
  314 |   await del.getByTestId("delete-submit").click();
  315 |   await expect(page).toHaveURL(/\/groups$/);
  316 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: `Deleted ${idle}.` })).toBeVisible();
  317 |   await expect(page.getByTestId("group-link").filter({ hasText: idle })).toHaveCount(0);
  318 |   // Its address now says there is no such group.
  319 |   await page.goto(`/groups/${idle}`);
  320 |   await expect(page.locator("h1")).toHaveText("No such group");
  321 | });
  322 | 
  323 | for (const width of [1024, 1280, 1440, 1920]) {
  324 |   test(`the Groups pages do not scroll sideways at ${width} px`, async ({ page }) => {
  325 |     await page.setViewportSize({ width, height: 900 });
  326 |     for (const path of ["/groups", `/groups/${member}`, `/groups/${member}/lag`, `/groups/${member}/lag?expiring=1`, `/groups/${member}/story`, `/groups/${member}/wire`, `/groups/${member}?dialog=reset`]) {
  327 |       await page.goto(path);
  328 |       await expect(page.locator("h1")).toBeVisible();
  329 |       await page.waitForLoadState("networkidle");
  330 |       expect(await overflowX(page), `${path} at ${width}`).toBeLessThanOrEqual(0);
  331 |     }
  332 |   });
  333 | }
  334 | 
```