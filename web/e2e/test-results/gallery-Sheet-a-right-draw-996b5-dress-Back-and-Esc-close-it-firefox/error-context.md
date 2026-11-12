# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: gallery.spec.ts >> Sheet: a right drawer the full height of the window, with its own address; Back and Esc close it
- Location: tests/gallery.spec.ts:118:1

# Error details

```
Error: expect(locator).toBeHidden() failed

Locator:  getByTestId('sheet')
Expected: hidden
Received: visible
Timeout:  8000ms

Call log:
  - Expect "toBeHidden" getByTestId('sheet') with timeout 8000ms
  - waiting for getByTestId('sheet')
    20 × locator resolved to <div role="dialog" tabindex="-1" id="radix-_r_19_" data-state="open" data-testid="sheet" data-slot="sheet-content" aria-labelledby="radix-_r_1a_" aria-describedby="radix-_r_1b_" class="fixed z-50 flex flex-col gap-4 bg-background shadow-lg transition ease-in-out data-[state=closed]:animate-out data-[state=closed]:duration-300 data-[state=open]:animate-in data-[state=open]:duration-500 inset-y-0 right-0 h-full border-l data-[state=closed]:slide-out-to-right data-[state=open]:slide-in-from-right w-fu…>…</div>
       - unexpected value "visible"

```

```yaml
- dialog "orders · 3 @ 1,000,021":
  - heading "orders · 3 @ 1,000,021" [level=2]
  - paragraph: "One record, completely: key, value, headers, both timestamps."
  - text: Key
  - code: ord-88403
  - 'button "Key: ord-88403"'
  - text: "Value {\"id\": 1000021, \"total\": 12.5}"
  - button "Close"
```

# Test source

```ts
  37  |   await page.getByPlaceholder("Search topics…").fill("pay");
  38  |   await page.getByRole("option", { name: /payments\.settled/ }).click();
  39  |   await expect(page.getByRole("combobox", { name: "Copy configs from" })).toHaveText(/payments\.settled/);
  40  |   // MultiSelect: two ticks, a chip each, one removed by its chip.
  41  |   await page.getByRole("combobox", { name: "Topics to watch" }).click();
  42  |   await page.getByRole("option", { name: /^orders\s/ }).click();
  43  |   await page.getByRole("option", { name: /clickstream/ }).click();
  44  |   await page.getByRole("option", { name: /pick-lists/ }).click();
  45  |   await page.keyboard.press("Escape");
  46  |   await expect(page.getByTestId("multi-summary")).toHaveText("3 topics");
  47  |   await page.getByRole("button", { name: "Remove pick-lists" }).click();
  48  |   await expect(page.getByTestId("multi-summary")).toHaveText("orders, clickstream");
  49  |   // RadioGroup, Switch, Checkbox.
  50  |   await page.getByRole("radio", { name: "1", exact: true }).click();
  51  |   await page.getByTestId("f-compact").click();
  52  |   await expect(page.getByTestId("f-compact")).toHaveAttribute("aria-checked", "true");
  53  |   await page.getByTestId("f-confirm").click();
  54  |   await expect(page.getByTestId("f-confirm")).toHaveAttribute("aria-checked", "true");
  55  |   await page.getByTestId("f-submit").click();
  56  |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Submitted" })).toBeVisible();
  57  |   const out = JSON.parse((await page.getByTestId("form-out").textContent()) ?? "{}");
  58  |   expect(out).toEqual({ name: "orders.v2", value: '{"id": 7}', partitions: "6", topic: "payments.settled", watch: ["orders", "clickstream"], compact: true, acks: "1", confirm: true });
  59  | });
  60  | 
  61  | test("Select and MultiSelect work with the keyboard alone", async ({ page }) => {
  62  |   await page.getByTestId("f-partitions").focus();
  63  |   await page.keyboard.press("Enter");
  64  |   await page.keyboard.press("ArrowDown");
  65  |   await page.keyboard.press("Enter");
  66  |   await expect(page.getByTestId("f-partitions")).toHaveText("6 partitions");
  67  |   await expect(page.getByTestId("f-partitions")).toBeFocused();
  68  |   const ms = page.getByRole("combobox", { name: "Topics to watch" });
  69  |   await ms.focus();
  70  |   await page.keyboard.press("Enter");
  71  |   await page.keyboard.type("dlq");
  72  |   await page.keyboard.press("Enter");
  73  |   await page.keyboard.press("Escape");
  74  |   await expect(page.getByTestId("multi-summary")).toHaveText("orders.dlq");
  75  |   await expect(ms).toBeFocused();
  76  | });
  77  | 
  78  | test("ToggleGroup and Tabs put their choice in the address, and Back restores it", async ({ page }) => {
  79  |   await page.getByTestId("density").getByRole("radio", { name: "Compact" }).click();
  80  |   await expect(page).toHaveURL(/density=compact/);
  81  |   await page.getByRole("tab", { name: "Partitions" }).click();
  82  |   await expect(page).toHaveURL(/tab=partitions/);
  83  |   await expect(page.getByRole("tabpanel")).toContainText("last stable offset");
  84  |   await page.getByRole("tab", { name: "Partitions" }).focus();
  85  |   await page.keyboard.press("ArrowRight");
  86  |   await expect(page).toHaveURL(/tab=config/);
  87  |   await expect(page.getByRole("tabpanel")).toContainText("where it comes from");
  88  |   await page.goBack();
  89  |   await expect(page).toHaveURL(/tab=partitions/);
  90  |   await expect(page.getByRole("tab", { name: "Partitions" })).toHaveAttribute("aria-selected", "true");
  91  |   await page.goto("/gallery?tab=config&density=compact");
  92  |   await expect(page.getByRole("tab", { name: "Config" })).toHaveAttribute("aria-selected", "true");
  93  |   await expect(page.getByTestId("density").getByRole("radio", { name: "Compact" })).toHaveAttribute("aria-checked", "true");
  94  | });
  95  | 
  96  | test("Dialog: opens, keeps the focus inside, closes with Esc to its opener, and its action works", async ({ page }) => {
  97  |   const opener = page.getByTestId("open-dialog");
  98  |   await opener.click();
  99  |   const dialog = page.getByTestId("dialog");
  100 |   await expect(dialog).toBeVisible();
  101 |   await expect(page.getByTestId("dialog-input")).toBeFocused();
  102 |   await expect(page.getByTestId("dialog-confirm")).toBeDisabled();
  103 |   for (let i = 0; i < 6; i++) await page.keyboard.press("Tab");
  104 |   expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  105 |   await page.keyboard.press("Escape");
  106 |   await expect(dialog).toBeHidden();
  107 |   await expect(opener).toBeFocused();
  108 |   await opener.click();
  109 |   await page.getByTestId("dialog-input").fill("orders.dlq");
  110 |   await page.getByTestId("dialog-confirm").click();
  111 |   await expect(dialog).toBeHidden();
  112 |   await expect(page.getByTestId("overlay-out")).toContainText("deleted: orders.dlq");
  113 |   await opener.click();
  114 |   await page.getByRole("button", { name: "Cancel" }).click();
  115 |   await expect(dialog).toBeHidden();
  116 | });
  117 | 
  118 | test("Sheet: a right drawer the full height of the window, with its own address; Back and Esc close it", async ({ page }) => {
  119 |   await page.getByTestId("open-sheet").click();
  120 |   const sheet = page.getByTestId("sheet");
  121 |   await expect(sheet).toBeVisible();
  122 |   await expect(page).toHaveURL(/record=r7/);
  123 |   await sheet.evaluate((e) => Promise.all(e.getAnimations({ subtree: true }).map((a) => a.finished)));
  124 |   const box = await sheet.boundingBox();
  125 |   const vp = page.viewportSize()!;
  126 |   expect(Math.round(box!.y)).toBe(0);
  127 |   expect(Math.round(box!.height)).toBe(vp.height);
  128 |   expect(Math.round(box!.x + box!.width)).toBe(vp.width);
  129 |   await page.goBack();
  130 |   await expect(sheet).toBeHidden();
  131 |   await expect(page).not.toHaveURL(/record=/);
  132 |   // From a table row, closed with Esc.
  133 |   await page.getByRole("row").filter({ hasText: "1,000,021" }).click();
  134 |   await expect(sheet).toBeVisible();
  135 |   await expect(page).toHaveURL(/record=r3/);
  136 |   await page.keyboard.press("Escape");
> 137 |   await expect(sheet).toBeHidden();
      |                       ^ Error: expect(locator).toBeHidden() failed
  138 |   await expect(page).not.toHaveURL(/record=/);
  139 |   // A deep link opens it; closing it stays on the gallery.
  140 |   await page.goto("/gallery?record=r5");
  141 |   await expect(sheet).toBeVisible();
  142 |   await page.getByRole("button", { name: "Close" }).click();
  143 |   await expect(sheet).toBeHidden();
  144 |   await expect(page).toHaveURL(/\/gallery$/);
  145 | });
  146 | 
  147 | test("DropdownMenu: a choice with the mouse, another with the keyboard", async ({ page }) => {
  148 |   await page.getByTestId("open-menu").click();
  149 |   await page.getByRole("menuitem", { name: "Add partitions" }).click();
  150 |   await expect(page.getByTestId("overlay-out")).toContainText("menu: Add partitions");
  151 |   await expect(page.getByTestId("open-menu")).toBeFocused();
  152 |   await page.keyboard.press("Enter");
  153 |   await expect(page.getByRole("menu")).toBeVisible();
  154 |   // The keyboard opens it on its first item (Produce): two down is Delete records.
  155 |   await expect(page.getByRole("menuitem", { name: "Produce" })).toBeFocused();
  156 |   await page.keyboard.press("ArrowDown");
  157 |   await page.keyboard.press("ArrowDown");
  158 |   await page.keyboard.press("Enter");
  159 |   await expect(page.getByTestId("overlay-out")).toContainText("menu: Delete records");
  160 | });
  161 | 
  162 | test("Popover: a field inside it applies, Esc closes it", async ({ page }) => {
  163 |   await page.getByTestId("open-popover").click();
  164 |   await page.getByTestId("popover-input").fill("2026-11-13 14:30");
  165 |   await page.getByTestId("popover-apply").click();
  166 |   await expect(page.getByTestId("popover-input")).toBeHidden();
  167 |   await expect(page.getByTestId("overlay-out")).toContainText("from: 2026-11-13 14:30");
  168 |   await page.getByTestId("open-popover").click();
  169 |   await expect(page.getByTestId("popover-input")).toBeVisible();
  170 |   await page.keyboard.press("Escape");
  171 |   await expect(page.getByTestId("popover-input")).toBeHidden();
  172 |   await expect(page.getByTestId("open-popover")).toBeFocused();
  173 | });
  174 | 
  175 | test("Tooltip: on hover and on keyboard focus", async ({ page }) => {
  176 |   await page.getByTestId("tooltip-trigger").hover();
  177 |   await expect(page.getByRole("tooltip")).toContainText("every replica");
  178 |   const h1 = await page.locator("h1").boundingBox();
  179 |   await page.mouse.move(h1!.x + 10, h1!.y + 10, { steps: 5 });
  180 |   await expect(page.getByRole("tooltip")).toBeHidden();
  181 |   await page.getByTestId("open-popover").focus();
  182 |   await page.keyboard.press("Tab");
  183 |   await expect(page.getByRole("tooltip")).toBeVisible();
  184 | });
  185 | 
  186 | test("Toast, StatStrip, CodeValue and EmptyState", async ({ page }) => {
  187 |   await page.getByTestId("toast-btn").click();
  188 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "slice R1" })).toBeVisible();
  189 |   const g = page.getByTestId("g-display");
  190 |   await expect(g.locator("[data-slot=stat]")).toHaveCount(3);
  191 |   const value = g.locator("[data-slot=stat-value]").nth(1);
  192 |   await expect(value).toHaveText("1,234,567");
  193 |   expect(await value.evaluate((e) => getComputedStyle(e).fontVariantNumeric)).toContain("tabular-nums");
  194 |   const code = g.locator("[data-slot=code-value] code");
  195 |   expect(await code.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
  196 |   await g.locator("[data-slot=code-value] button").click();
  197 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Path copied" })).toBeVisible();
  198 |   await expect(g.locator("[data-slot=empty-state]")).toContainText("No records yet");
  199 | });
  200 | 
  201 | test("DataTable: fixed columns, ellipsis with the whole value, sorting, a sticky header, and a virtual body past 500 rows", async ({ page }) => {
  202 |   const table = page.getByTestId("g-table").locator("[data-slot=data-table]");
  203 |   await expect(table.locator("tbody tr[data-row]")).toHaveCount(20);
  204 |   const key = table.locator("tbody tr[data-row='r0'] td").nth(3);
  205 |   expect(await key.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
  206 |   await expect(key).toHaveAttribute("title", /with-a-very-long-key/);
  207 |   const widths = await table.locator("thead th").evaluateAll((ths) => ths.map((t) => Math.round(t.getBoundingClientRect().width)));
  208 |   // Sort by Bytes (descending on the second click): the rows reorder, the columns do not move.
  209 |   const bytes = table.getByRole("button", { name: "Bytes" });
  210 |   const sorted = async (dir: "ascending" | "descending") => {
  211 |     await expect(table.locator("thead th").nth(4)).toHaveAttribute("aria-sort", dir);
  212 |     const nums = (await table.locator("tbody tr[data-row] td:nth-child(5)").allTextContents()).map(Number);
  213 |     expect(nums).toEqual([...nums].sort((a, b) => (dir === "ascending" ? a - b : b - a)));
  214 |   };
  215 |   await bytes.click();
  216 |   await sorted("descending");
  217 |   await bytes.click();
  218 |   await sorted("ascending");
  219 |   expect(await table.locator("thead th").evaluateAll((ths) => ths.map((t) => Math.round(t.getBoundingClientRect().width)))).toEqual(widths);
  220 |   // 1,000 rows: virtual (far fewer rows in the DOM), the header stays at the top while the body scrolls.
  221 |   await page.getByTestId("rows").getByRole("radio", { name: "1,000" }).click();
  222 |   await expect(page).toHaveURL(/rows=1000/);
  223 |   await expect(table).toHaveAttribute("data-virtual", "true");
  224 |   const inDom = await table.locator("tbody tr[data-row]").count();
  225 |   expect(inDom).toBeGreaterThan(5);
  226 |   expect(inDom).toBeLessThan(80);
  227 |   const headTop = await table.locator("thead").evaluate((h) => h.getBoundingClientRect().top);
  228 |   await table.evaluate((t) => (t.scrollTop = 20_000));
  229 |   await expect(table.locator("tbody tr[data-row='r520']")).toBeVisible();
  230 |   expect(await table.locator("thead").evaluate((h) => h.getBoundingClientRect().top)).toBe(headTop);
  231 |   expect(await table.locator("tbody tr[data-row]").count()).toBeLessThan(80);
  232 | });
  233 | 
```