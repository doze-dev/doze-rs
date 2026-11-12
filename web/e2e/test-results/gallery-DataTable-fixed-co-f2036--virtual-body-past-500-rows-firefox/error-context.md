# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: gallery.spec.ts >> DataTable: fixed columns, ellipsis with the whole value, sorting, a sticky header, and a virtual body past 500 rows
- Location: tests/gallery.spec.ts:201:1

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByTestId('g-table').locator('[data-slot=data-table]').locator('tbody tr[data-row=\'r520\']')
Expected: visible
Timeout: 8000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" getByTestId('g-table').locator('[data-slot=data-table]').locator('tbody tr[data-row=\'r520\']') with timeout 8000ms
  - waiting for getByTestId('g-table').locator('[data-slot=data-table]').locator('tbody tr[data-row=\'r520\']')

```

```yaml
- link "doze-kafka, Home":
  - /url: /
  - text: doze-kafka
- list:
  - listitem:
    - link "Home":
      - /url: /
  - listitem:
    - link "Live":
      - /url: /live
  - listitem:
    - link "Topics":
      - /url: /topics
    - text: "3"
  - listitem:
    - link "Groups":
      - /url: /groups
    - text: "1"
  - listitem:
    - link "Access":
      - /url: /access
  - listitem:
    - link "Lab":
      - /url: /lab
  - listitem:
    - link "Settings":
      - /url: /settings
- list:
  - listitem:
    - link "Classic console":
      - /url: /classic/
- button "Toggle Sidebar"
- main:
  - button "Toggle Sidebar"
  - button "Jump to a topic or group ⌘K"
  - button "127.0.0.1:39092"
  - text: Kafka 4 profile
  - link "Open":
    - /url: /access
  - main:
    - navigation "Breadcrumb":
      - list:
        - listitem:
          - link "Home":
            - /url: /
    - heading "Component gallery" [level=1]
    - paragraph: Every control the console is made of, each doing its real job. The browser tests drive this page in Firefox, WebKit and Chromium.
    - text: Button Variants and sizes; the count proves a click lands.
    - button "Primary"
    - button "Outline"
    - button "Secondary"
    - button "Ghost"
    - button "Delete"
    - button "Link"
    - button "Disabled" [disabled]
    - button "Small"
    - status: "clicks: 0"
    - text: "ToggleGroup (segmented) and Tabs Both are in the address: Back restores them. Density"
    - radiogroup "Density":
      - radio "Comfortable" [checked]
      - radio "Compact"
    - tablist:
      - tab "Messages" [selected]
      - tab "Partitions"
      - tab "Config"
    - tabpanel "Messages": The newest records of the topic.
    - text: "Form: Input, Textarea, Select, Combobox, MultiSelect, RadioGroup, Switch, Checkbox react-hook-form with a zod schema: the errors are Kafka's rules, the submit shows what was sent. Topic name"
    - textbox "Topic name":
      - /placeholder: orders
    - paragraph: "Kafka's rule: letters, digits, '.', '_' and '-'."
    - text: Partitions
    - combobox "Partitions": 3 partitions
    - text: Copy configs from
    - combobox "Copy configs from": Choose a topic…
    - text: Topics to watch
    - combobox "Topics to watch": All topics
    - text: First record's value
    - textbox "First record's value":
      - /placeholder: "{\"id\": 1}"
    - text: acks
    - radiogroup:
      - radio "0"
      - text: "0"
      - radio "1"
      - text: "1"
      - radio "all" [checked]
      - text: all
    - text: Compacted
    - paragraph: cleanup.policy=compact
    - switch "Compacted"
    - checkbox "Create it now"
    - text: Create it now
    - button "Create topic"
    - button "Reset"
    - status: nothing submitted
    - text: Dialog, Sheet, DropdownMenu, Popover, Tooltip Esc closes, the focus returns to the opener; the Sheet has its own address (Back closes it).
    - button "Delete topic…"
    - button "Open record r7"
    - button "Row actions"
    - button "From time"
    - button "What is a high watermark?"
    - status: "deleted: none · menu: none · from: none"
    - text: Card, Badge, status, StatStrip, CodeValue, EmptyState, Toast New compact v3 refused Running Rebalancing Not reachable Not checked
    - term: Topics
    - definition: "7"
    - text: 21 partitions
    - term: Records
    - definition: 1,234,567
    - text: in the log
    - term: Last request
    - definition: 14:32:07
    - text: UTC
    - code: /Users/srini/Library/Application Support/doze/kafka/data/orders-0/00000000000000000000.log
    - 'button "Path: /Users/srini/Library/Application Support/doze/kafka/data/orders-0/00000000000000000000.log"'
    - heading "No records yet" [level=3]
    - text: This topic is empty. Produce one, or point a client at it.
    - button "Produce a record"
    - text: DataTable Fixed columns, ellipsis with the whole value on hover, sticky header, stable row ids; virtual past 500 rows. Rows
    - radiogroup "Rows":
      - radio "20"
      - radio "1,000" [checked]
    - table "Records":
      - rowgroup:
        - row "Topic Partition Offset Key Bytes Transaction":
          - columnheader "Topic":
            - button "Topic"
          - columnheader "Partition":
            - button "Partition"
          - columnheader "Offset":
            - button "Offset"
          - columnheader "Key":
            - button "Key"
          - columnheader "Bytes":
            - button "Bytes"
          - columnheader "Transaction":
            - button "Transaction"
      - rowgroup:
        - row "payments.settled 0 1,003,318 ord-88874 558 committed":
          - cell "payments.settled"
          - cell "0"
          - cell "1,003,318"
          - cell "ord-88874"
          - cell "558"
          - cell "committed"
        - row "inventory.levels 1 1,003,829 ord-88947 559 committed":
          - cell "inventory.levels"
          - cell "1"
          - cell "1,003,829"
          - cell "ord-88947"
          - cell "559"
          - cell "committed"
        - row "notifications.email 2 1,004,340 ord-89020 560 committed":
          - cell "notifications.email"
          - cell "2"
          - cell "1,004,340"
          - cell "ord-89020"
          - cell "560"
          - cell "committed"
        - row "clickstream 3 1,004,851 ord-693-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 561 committed":
          - cell "clickstream"
          - cell "3"
          - cell "1,004,851"
          - cell "ord-693-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          - cell "561"
          - cell "committed"
        - row "pick-lists 4 1,005,362 ord-89166 562 committed":
          - cell "pick-lists"
          - cell "4"
          - cell "1,005,362"
          - cell "ord-89166"
          - cell "562"
          - cell "committed"
        - row "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2 5 1,005,873 ord-89239 563 committed":
          - cell "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"
          - cell "5"
          - cell "1,005,873"
          - cell "ord-89239"
          - cell "563"
          - cell "committed"
        - row "notifications.email 0 1,000,084 ord-88412 564 committed":
          - cell "notifications.email"
          - cell "0"
          - cell "1,000,084"
          - cell "ord-88412"
          - cell "564"
          - cell "committed"
        - row "orders 0 1,006,384 ord-89312 564 committed":
          - cell "orders"
          - cell "0"
          - cell "1,006,384"
          - cell "ord-89312"
          - cell "564"
          - cell "committed"
        - row "clickstream 1 1,000,595 ord-88485 565 open":
          - cell "clickstream"
          - cell "1"
          - cell "1,000,595"
          - cell "ord-88485"
          - cell "565"
          - cell "open"
        - row "orders.dlq 1 1,006,895 ord-89385 565 committed":
          - cell "orders.dlq"
          - cell "1"
          - cell "1,006,895"
          - cell "ord-89385"
          - cell "565"
          - cell "committed"
        - row "pick-lists 2 1,001,106 ord-88558 566 committed":
          - cell "pick-lists"
          - cell "2"
          - cell "1,001,106"
          - cell "ord-88558"
          - cell "566"
          - cell "committed"
        - row "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2 3 1,001,617 ord-88631 567 committed":
          - cell "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"
          - cell "3"
          - cell "1,001,617"
          - cell "ord-88631"
          - cell "567"
          - cell "committed"
        - row "orders 4 1,002,128 ord-88704 568 committed":
          - cell "orders"
          - cell "4"
          - cell "1,002,128"
          - cell "ord-88704"
          - cell "568"
          - cell "committed"
        - row "orders.dlq 5 1,002,639 ord-88777 569 committed":
          - cell "orders.dlq"
          - cell "5"
          - cell "1,002,639"
          - cell "ord-88777"
          - cell "569"
          - cell "committed"
        - row "payments.settled 0 1,003,150 ord-450-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 570 committed":
          - cell "payments.settled"
          - cell "0"
          - cell "1,003,150"
          - cell "ord-450-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          - cell "570"
          - cell "committed"
        - row "inventory.levels 1 1,003,661 ord-88923 571 committed":
          - cell "inventory.levels"
          - cell "1"
          - cell "1,003,661"
          - cell "ord-88923"
          - cell "571"
          - cell "committed"
        - row "notifications.email 2 1,004,172 ord-88996 572 committed":
          - cell "notifications.email"
          - cell "2"
          - cell "1,004,172"
          - cell "ord-88996"
          - cell "572"
          - cell "committed"
        - row "clickstream 3 1,004,683 ord-89069 573 committed":
          - cell "clickstream"
          - cell "3"
          - cell "1,004,683"
          - cell "ord-89069"
          - cell "573"
          - cell "committed"
        - row "pick-lists 4 1,005,194 ord-89142 574 committed":
          - cell "pick-lists"
          - cell "4"
          - cell "1,005,194"
          - cell "ord-89142"
          - cell "574"
          - cell "committed"
        - row "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2 5 1,005,705 ord-89215 575 committed":
          - cell "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"
          - cell "5"
          - cell "1,005,705"
          - cell "ord-89215"
          - cell "575"
          - cell "committed"
        - row "orders 0 1,006,216 ord-89288 576 committed":
          - cell "orders"
          - cell "0"
          - cell "1,006,216"
          - cell "ord-89288"
          - cell "576"
          - cell "committed"
        - row "clickstream 1 1,000,427 ord-88461 577 committed":
          - cell "clickstream"
          - cell "1"
          - cell "1,000,427"
          - cell "ord-88461"
          - cell "577"
          - cell "committed"
        - row "orders.dlq 1 1,006,727 ord-89361 577 committed":
          - cell "orders.dlq"
          - cell "1"
          - cell "1,006,727"
          - cell "ord-89361"
          - cell "577"
          - cell "committed"
        - row "pick-lists 2 1,000,938 ord-88534 578 committed":
          - cell "pick-lists"
          - cell "2"
          - cell "1,000,938"
          - cell "ord-88534"
          - cell "578"
          - cell "committed"
        - row "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2 3 1,001,449 ord-207-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 579 aborted":
          - cell "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"
          - cell "3"
          - cell "1,001,449"
          - cell "ord-207-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          - cell "579"
          - cell "aborted"
        - row "orders 4 1,001,960 ord-88680 580 committed":
          - cell "orders"
          - cell "4"
          - cell "1,001,960"
          - cell "ord-88680"
          - cell "580"
          - cell "committed"
        - row "orders.dlq 5 1,002,471 ord-88753 581 committed":
          - cell "orders.dlq"
          - cell "5"
          - cell "1,002,471"
          - cell "ord-88753"
          - cell "581"
          - cell "committed"
        - row "payments.settled 0 1,002,982 ord-88826 582 committed":
          - cell "payments.settled"
          - cell "0"
          - cell "1,002,982"
          - cell "ord-88826"
          - cell "582"
          - cell "committed"
        - row "inventory.levels 1 1,003,493 ord-88899 583 committed":
          - cell "inventory.levels"
          - cell "1"
          - cell "1,003,493"
          - cell "ord-88899"
          - cell "583"
          - cell "committed"
        - row "notifications.email 2 1,004,004 ord-88972 584 committed":
          - cell "notifications.email"
          - cell "2"
          - cell "1,004,004"
          - cell "ord-88972"
          - cell "584"
          - cell "committed"
        - row "clickstream 3 1,004,515 ord-89045 585 committed":
          - cell "clickstream"
          - cell "3"
          - cell "1,004,515"
          - cell "ord-89045"
          - cell "585"
          - cell "committed"
        - row "pick-lists 4 1,005,026 ord-89118 586 committed":
          - cell "pick-lists"
          - cell "4"
          - cell "1,005,026"
          - cell "ord-89118"
          - cell "586"
          - cell "committed"
        - row "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2 5 1,005,537 ord-89191 587 committed":
          - cell "a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"
          - cell "5"
          - cell "1,005,537"
          - cell "ord-89191"
          - cell "587"
          - cell "committed"
        - row "orders 0 1,006,048 ord-864-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx 588 committed":
          - cell "orders"
          - cell "0"
          - cell "1,006,048"
          - cell "ord-864-with-a-very-long-key-that-will-be-cut-by-the-column-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
          - cell "588"
          - cell "committed"
        - row "clickstream 1 1,000,259 ord-88437 589 committed":
          - cell "clickstream"
          - cell "1"
          - cell "1,000,259"
          - cell "ord-88437"
          - cell "589"
          - cell "committed"
  - heading "Jump to a topic or group" [level=2]
  - paragraph: Type a name, then Enter
- region "Notifications alt+T"
```

# Test source

```ts
  129 |   await page.goBack();
  130 |   await expect(sheet).toBeHidden();
  131 |   await expect(page).not.toHaveURL(/record=/);
  132 |   // From a table row, closed with Esc.
  133 |   await page.getByRole("row").filter({ hasText: "1,000,021" }).click();
  134 |   await expect(sheet).toBeVisible();
  135 |   await expect(page).toHaveURL(/record=r3/);
  136 |   await page.keyboard.press("Escape");
  137 |   await expect(sheet).toBeHidden();
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
> 229 |   await expect(table.locator("tbody tr[data-row='r520']")).toBeVisible();
      |                                                            ^ Error: expect(locator).toBeVisible() failed
  230 |   expect(await table.locator("thead").evaluate((h) => h.getBoundingClientRect().top)).toBe(headTop);
  231 |   expect(await table.locator("tbody tr[data-row]").count()).toBeLessThan(80);
  232 | });
  233 | 
```