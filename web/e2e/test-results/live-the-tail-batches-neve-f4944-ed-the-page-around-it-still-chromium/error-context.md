# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: live.spec.ts >> the tail: batches never faster than the pace, existing rows never rewritten, new rows highlighted, the page around it still
- Location: tests/live.spec.ts:118:1

# Error details

```
Test timeout of 45000ms exceeded.
```

```
Error: the traffic made batches

expect(received).toBeGreaterThanOrEqual(expected)

Expected: >= 2
Received:    0
```

# Test source

```ts
  50  |   const k = new Kafka({ clientId: "admin-cli", brokers: [broker], logLevel: logLevel.NOTHING, retry: { retries: 0 } });
  51  |   const admin = k.admin();
  52  |   await admin.connect();
  53  |   await admin.createTopics({ topics: [{ topic: "inventory.levels", numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact" }] }] }).catch(() => {});
  54  |   await admin.disconnect();
  55  |   const p = k.producer();
  56  |   await p.connect();
  57  |   await p.send({ topic: "inventory.levels", messages: [{ value: "no key" }] }).catch(() => {});
  58  |   await p.disconnect();
  59  | }
  60  | 
  61  | let stopTraffic: (() => Promise<void>) | undefined;
  62  | test.beforeAll(async () => {
  63  |   await refusedProduce(KAFKA);
  64  |   stopTraffic = traffic(KAFKA);
  65  |   await sleep(3000);
  66  | });
  67  | test.afterAll(async () => {
  68  |   await stopTraffic?.();
  69  | });
  70  | 
  71  | // A fresh browser state for each test, before its first page (a second navigation right after the first cut the first
  72  | // page's requests short, which WebKit reports as errors).
  73  | test.beforeEach(async ({ page }) => {
  74  |   await page.addInitScript(() => {
  75  |     if (!sessionStorage.getItem("r3-clean")) {
  76  |       localStorage.clear();
  77  |       sessionStorage.setItem("r3-clean", "1");
  78  |     }
  79  |   });
  80  | });
  81  | 
  82  | const tape = (page: Page) => page.getByTestId("tape");
  83  | const rowsOf = (page: Page) => tape(page).locator("tbody tr[data-row]");
  84  | const scroller = (page: Page) => tape(page).locator("[data-slot=data-table]");
  85  | 
  86  | async function openLive(page: Page, search = "") {
  87  |   await page.goto(`/live${search}`);
  88  |   await expect(page.locator("h1")).toHaveText("Live");
  89  |   await expect(rowsOf(page).first()).toBeVisible();
  90  | }
  91  | 
  92  | async function tailOn(page: Page) {
  93  |   await page.getByTestId("tail-mode").getByRole("radio", { name: "Live tail" }).click();
  94  |   await expect(page).toHaveURL(/tail=/);
  95  |   await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  96  |   // The pointer off the tape (hovering holds it).
  97  |   await page.mouse.move(2, 600);
  98  | }
  99  | 
  100 | test("Live is a snapshot by default: one h1, the figures in tiles, Snapshot chosen, and no request after it loaded", async ({ page }) => {
  101 |   await openLive(page);
  102 |   await expect(page.locator("h1")).toHaveCount(1);
  103 |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("");
  104 |   const strip = page.locator("[data-slot=stat-strip]");
  105 |   for (const label of ["Requests, last minute", "Rows kept", "Reaches back to", "Clients"]) await expect(strip).toContainText(label);
  106 |   await expect(strip).toContainText("of 2,000");
  107 |   await expect(page.getByTestId("tail-mode").getByRole("radio", { name: "Snapshot" })).toHaveAttribute("aria-checked", "true");
  108 |   await expect(page.getByTestId("tail-state")).toHaveText("Snapshot");
  109 |   await expect(page.getByTestId("tail-speed")).toHaveText(/Relaxed · 3 s/);
  110 |   const asked: string[] = [];
  111 |   page.on("request", (r) => asked.push(r.url()));
  112 |   const before = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  113 |   await sleep(6000);
  114 |   expect(asked, "no request after load").toEqual([]);
  115 |   expect(await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")))).toEqual(before);
  116 | });
  117 | 
  118 | test("the tail: batches never faster than the pace, existing rows never rewritten, new rows highlighted, the page around it still", async ({ page }) => {
  119 |   await openLive(page);
  120 |   await tailOn(page);
  121 |   const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=refusals]", "[data-testid=live-toolbar]", "[data-testid=tail-bar]", "thead"];
  122 |   const before = await boxes(page, selectors);
  123 |   await page.evaluate(() => {
  124 |     const w = window as unknown as { __batches: number[]; __old: Map<string, [Element, string]>; __fresh: number };
  125 |     w.__batches = [];
  126 |     w.__fresh = 0;
  127 |     w.__old = new Map();
  128 |     for (const tr of document.querySelectorAll("[data-testid=tape] tbody tr[data-row]")) w.__old.set(tr.getAttribute("data-row")!, [tr, tr.outerHTML]);
  129 |     new MutationObserver((records) => {
  130 |       const added = records.some((r) => [...r.addedNodes].some((n) => n instanceof HTMLElement && n.matches("tr[data-row]")));
  131 |       if (added) w.__batches.push(performance.now());
  132 |     }).observe(document.querySelector("[data-testid=tape] tbody")!, { childList: true });
  133 |   });
  134 |   await sleep(20_000);
  135 |   const r = await page.evaluate(() => {
  136 |     const w = window as unknown as { __batches: number[]; __old: Map<string, [Element, string]> };
  137 |     let replaced = 0;
  138 |     let rewritten = 0;
  139 |     for (const [id, [node, html]] of w.__old) {
  140 |       const now = document.querySelector(`[data-testid=tape] tbody tr[data-row="${id}"]`);
  141 |       if (!now) continue; // fell off the bottom of the 200
  142 |       if (now !== node) replaced++;
  143 |       // A folded repeat that grew changes its own count and time in place, by design; nothing else may change.
  144 |       else if (now.outerHTML !== html && !now.textContent!.includes("×")) rewritten++;
  145 |     }
  146 |     const fresh = document.querySelectorAll("[data-testid=tape] tbody tr:has([data-fresh])").length;
  147 |     const freshOld = [...w.__old.values()].filter(([n]) => n.querySelector("[data-fresh]")).length;
  148 |     return { batches: w.__batches, replaced, rewritten, fresh, freshOld };
  149 |   });
> 150 |   expect(r.batches.length, "the traffic made batches").toBeGreaterThanOrEqual(2);
      |                                                        ^ Error: the traffic made batches
  151 |   const gaps = r.batches.slice(1).map((t, i) => t - r.batches[i]!);
  152 |   for (const g of gaps) expect(g, `gaps ${gaps.map(Math.round).join(", ")} ms`).toBeGreaterThanOrEqual(2900);
  153 |   expect(r.replaced, "existing rows replaced").toBe(0);
  154 |   expect(r.rewritten, "existing rows rewritten").toBe(0);
  155 |   expect(r.fresh, "new rows carry the highlight").toBeGreaterThan(0);
  156 |   expect(r.freshOld, "rows of the snapshot never highlight").toBe(0);
  157 |   expect(await boxes(page, selectors)).toEqual(before);
  158 |   // No row twice.
  159 |   const ids = await rowsOf(page).evaluateAll((rs) => rs.map((x) => x.getAttribute("data-row")));
  160 |   expect(new Set(ids).size).toBe(ids.length);
  161 | });
  162 | 
  163 | test("the tail holds while the tape is scrolled: the pill counts, the scroll stays, Show applies", async ({ page }) => {
  164 |   await openLive(page);
  165 |   await choose(page, page.getByTestId("tail-speed"), "Realtime · 1 s");
  166 |   expect(await page.evaluate(() => localStorage.getItem("doze:tail-speed"))).toBe("realtime");
  167 |   await tailOn(page);
  168 |   await expect(rowsOf(page).nth(16)).toBeAttached();
  169 |   await scroller(page).hover();
  170 |   await page.mouse.wheel(0, 240);
  171 |   await page.mouse.move(2, 600);
  172 |   const pill = page.getByTestId("held-pill");
  173 |   await expect(pill).toBeVisible({ timeout: 10_000 });
  174 |   const at = async () => ({ top: await scroller(page).evaluate((e) => e.scrollTop), y: await page.evaluate(() => window.scrollY), first: await rowsOf(page).first().getAttribute("data-row"), n: await rowsOf(page).count() });
  175 |   const held = await at();
  176 |   expect(held.top).toBeGreaterThan(100);
  177 |   const count1 = Number((await pill.innerText()).match(/\d+/)![0]);
  178 |   expect(count1).toBeGreaterThan(0);
  179 |   await sleep(3500);
  180 |   expect(await at()).toEqual(held);
  181 |   const count2 = Number((await pill.innerText()).match(/\d+/)![0]);
  182 |   expect(count2).toBeGreaterThanOrEqual(count1);
  183 |   await pill.click();
  184 |   await expect(pill).toBeHidden();
  185 |   const after = await at();
  186 |   expect(after.top).toBe(0);
  187 |   expect(after.first).not.toBe(held.first);
  188 |   expect(after.y).toBe(held.y);
  189 | });
  190 | 
  191 | test("filters change the rows, live in the address, and survive a reload", async ({ page }) => {
  192 |   await openLive(page);
  193 |   // Topics: a MultiSelect with counts.
  194 |   await page.getByRole("combobox", { name: "Topics" }).click();
  195 |   await page.getByRole("option", { name: /^orders \d/ }).click();
  196 |   await page.keyboard.press("Escape");
  197 |   await expect(page).toHaveURL(/topic=/);
  198 |   await expect(page.getByRole("combobox", { name: "Topics" })).toHaveText("orders");
  199 |   await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(4)").allInnerTexts()).every((t) => t.includes("orders"))).toBe(true);
  200 |   // Refused only.
  201 |   await page.getByTestId("refused-only").click();
  202 |   await expect(page).toHaveURL(/refused=/);
  203 |   await expect(page.getByTestId("refused-only")).toHaveAttribute("data-state", "on");
  204 |   await page.reload();
  205 |   await expect(page.getByTestId("refused-only")).toHaveAttribute("data-state", "on");
  206 |   await expect(page.getByRole("combobox", { name: "Topics" })).toHaveText("orders");
  207 |   await page.getByTestId("refused-only").click();
  208 |   await expect(page).not.toHaveURL(/refused=/);
  209 |   // Idle polls: shown when switched off.
  210 |   await expect(page.getByTestId("pager")).toContainText("idle polls hidden");
  211 |   await page.getByTestId("idle-hidden").click();
  212 |   await expect(page).toHaveURL(/idle=/);
  213 |   await expect(page.getByTestId("pager")).not.toContainText("idle polls hidden");
  214 |   // A search that matches nothing says which filters are on and offers the way out.
  215 |   await page.getByTestId("live-search").fill("no-such-thing-anywhere");
  216 |   await expect(page).toHaveURL(/q=no-such-thing-anywhere/);
  217 |   await expect(page.getByTestId("live-no-match")).toContainText("topic is orders");
  218 |   await page.getByTestId("live-no-match").getByRole("button", { name: "Clear filters" }).click();
  219 |   await expect(page).not.toHaveURL(/topic=|q=/);
  220 |   await expect(rowsOf(page).first()).toBeVisible();
  221 |   // A click on a client in a row filters by it.
  222 |   const client = (await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().innerText()).trim();
  223 |   await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().click();
  224 |   await expect(page).toHaveURL(new RegExp(`client=`));
  225 |   await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()).every((t) => t.includes(client))).toBe(true);
  226 |   await page.goBack();
  227 |   await expect(page).not.toHaveURL(/client=/);
  228 | });
  229 | 
  230 | test("a request opens in a Sheet with its own address: Esc closes, Back closes, a deep link opens it", async ({ page }) => {
  231 |   await openLive(page);
  232 |   const first = rowsOf(page).first();
  233 |   const id = await first.getAttribute("data-row");
  234 |   await first.locator("td").nth(4).click();
  235 |   await expect(page).toHaveURL(new RegExp(`request=${id}`));
  236 |   const sheet = page.getByTestId("request-sheet");
  237 |   await expect(sheet).toBeVisible();
  238 |   await expect(sheet.getByTestId("request-body")).toContainText(/Correlation id|Kafka's words/);
  239 |   await page.keyboard.press("Escape");
  240 |   await expect(sheet).toBeHidden();
  241 |   await expect(page).not.toHaveURL(/request=/);
  242 |   await first.locator("td").nth(4).click();
  243 |   await expect(sheet).toBeVisible();
  244 |   await page.goBack();
  245 |   await expect(sheet).toBeHidden();
  246 |   await expect(page).not.toHaveURL(/request=/);
  247 |   await page.goto(`/live?request=${id}`);
  248 |   await expect(sheet).toBeVisible();
  249 |   await sheet.getByRole("button", { name: "Close" }).click();
  250 |   await expect(sheet).toBeHidden();
```