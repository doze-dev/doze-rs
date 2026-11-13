# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: live.spec.ts >> no page scrolls sideways at 1024 to 1920, in a snapshot, in the tail and under a Sheet
- Location: tests/live.spec.ts:302:1

# Error details

```
Error: 1024 table

1024 table

expect(received).toBe(expected) // Object.is equality

Expected: 0
Received: 82

Call Log:
- Timeout 8000ms exceeded while waiting on the predicate
```

# Test source

```ts
  207 |   await expect(page).toHaveURL(/idle=/);
  208 |   await expect(page.getByTestId("pager")).not.toContainText("idle polls hidden");
  209 |   // A search that matches nothing says which filters are on and offers the way out.
  210 |   await page.getByTestId("live-search").fill("no-such-thing-anywhere");
  211 |   await expect(page).toHaveURL(/q=no-such-thing-anywhere/);
  212 |   await expect(page.getByTestId("live-no-match")).toContainText("topic is orders");
  213 |   await page.getByTestId("live-no-match").getByRole("button", { name: "Clear filters" }).click();
  214 |   await expect(page).not.toHaveURL(/topic=|q=/);
  215 |   await expect(rowsOf(page).first()).toBeVisible();
  216 |   // A click on a client in a row filters by it.
  217 |   const client = (await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().innerText()).trim();
  218 |   await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().click();
  219 |   await expect(page).toHaveURL(new RegExp(`client=`));
  220 |   for (const t of await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()) expect(t).toContain(client);
  221 |   await page.goBack();
  222 |   await expect(page).not.toHaveURL(/client=/);
  223 | });
  224 | 
  225 | test("a request opens in a Sheet with its own address: Esc closes, Back closes, a deep link opens it", async ({ page }) => {
  226 |   await openLive(page);
  227 |   const first = rowsOf(page).first();
  228 |   const id = await first.getAttribute("data-row");
  229 |   await first.locator("td").nth(4).click();
  230 |   await expect(page).toHaveURL(new RegExp(`request=${id}`));
  231 |   const sheet = page.getByTestId("request-sheet");
  232 |   await expect(sheet).toBeVisible();
  233 |   await expect(sheet.getByTestId("request-body")).toContainText(/Correlation id|Kafka's words/);
  234 |   await page.keyboard.press("Escape");
  235 |   await expect(sheet).toBeHidden();
  236 |   await expect(page).not.toHaveURL(/request=/);
  237 |   await first.locator("td").nth(4).click();
  238 |   await expect(sheet).toBeVisible();
  239 |   await page.goBack();
  240 |   await expect(sheet).toBeHidden();
  241 |   await expect(page).not.toHaveURL(/request=/);
  242 |   await page.goto(`/live?request=${id}`);
  243 |   await expect(sheet).toBeVisible();
  244 |   await sheet.getByRole("button", { name: "Close" }).click();
  245 |   await expect(sheet).toBeHidden();
  246 |   await expect(page).toHaveURL(/\/live$/);
  247 |   // A request that is not on the tape says so.
  248 |   await page.goto("/live?request=987654321");
  249 |   await expect(sheet.getByTestId("request-gone")).toContainText("no longer on the wire");
  250 | });
  251 | 
  252 | test("a refused request: Kafka's words, the rule behind them, what would pass", async ({ page }) => {
  253 |   await openLive(page, "?refused=1");
  254 |   const row = rowsOf(page).filter({ hasText: "INVALID_RECORD" }).first();
  255 |   await expect(row).toBeVisible();
  256 |   await row.locator("td").nth(4).click();
  257 |   const sheet = page.getByTestId("request-sheet");
  258 |   await expect(sheet).toHaveAttribute("data-kind", "refused");
  259 |   await expect(sheet.getByRole("heading", { name: "INVALID_RECORD (87)" })).toBeVisible();
  260 |   await expect(sheet.getByTestId("kafka-words")).toContainText("Kafka's words");
  261 |   await expect(sheet.getByTestId("rules")).toContainText("cleanup.policy=compact");
  262 |   await expect(sheet.getByText("Give it a key")).toBeVisible();
  263 |   // The refusals strip opens to one line per (code, client).
  264 |   await page.keyboard.press("Escape");
  265 |   await page.getByTestId("refusals").getByRole("button").first().click();
  266 |   await expect(page.getByTestId("refusal-lines")).toContainText("INVALID_RECORD (87)");
  267 | });
  268 | 
  269 | test("the clients Sheet: who is connected, and a name filters the tape", async ({ page }) => {
  270 |   await openLive(page);
  271 |   await page.getByTestId("open-clients").click();
  272 |   await expect(page).toHaveURL(/clients=/);
  273 |   const sheet = page.getByTestId("clients-sheet");
  274 |   await expect(sheet.getByTestId("clients")).toContainText("e2e-orders");
  275 |   await page.keyboard.press("Escape");
  276 |   await expect(page).not.toHaveURL(/clients=/);
  277 |   await page.getByTestId("open-clients").click();
  278 |   await sheet.getByRole("link", { name: "e2e-orders" }).click();
  279 |   await expect(sheet).toBeHidden();
  280 |   await expect(page).toHaveURL(/client=/);
  281 |   for (const t of await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()) expect(t).toContain("e2e-orders");
  282 | });
  283 | 
  284 | test("the first-request page and the docks", async ({ page }) => {
  285 |   await page.goto("/connect/first-request");
  286 |   await expect(page.locator("h1")).toHaveText("A new client just connected");
  287 |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText(/Home.*Connect/);
  288 |   await expect(page.getByTestId("first-banner")).toBeVisible();
  289 |   await expect(page.getByTestId("first-story").locator("li").first()).toContainText(/ApiVersions|Metadata/);
  290 |   await page.goto("/gallery");
  291 |   const dock = page.locator("[data-testid=wire-dock][data-topic=orders]");
  292 |   await expect(dock.locator("tbody tr[data-row]").first()).toBeVisible();
  293 |   for (const t of await dock.locator("tbody tr[data-row] td:nth-child(4)").allInnerTexts()) expect(t).toContain("orders");
  294 |   const group = page.locator("[data-testid=wire-dock][data-group=fulfilment]");
  295 |   await group.getByRole("radio", { name: "Commits" }).click();
  296 |   await expect(group.getByRole("radio", { name: "Commits" })).toHaveAttribute("aria-checked", "true");
  297 |   await dock.locator("tbody tr[data-row]").first().getByTestId("row-time").click();
  298 |   await expect(page).toHaveURL(/\/live\?request=\d+/);
  299 |   await expect(page.getByTestId("request-sheet")).toBeVisible();
  300 | });
  301 | 
  302 | test("no page scrolls sideways at 1024 to 1920, in a snapshot, in the tail and under a Sheet", async ({ page }) => {
  303 |   for (const w of [1024, 1280, 1440, 1920]) {
  304 |     await page.setViewportSize({ width: w, height: 900 });
  305 |     await openLive(page);
  306 |     expect(await overflowX(page), `${w} snapshot`).toBe(0);
> 307 |     await expect.poll(() => scroller(page).evaluate((e) => e.scrollWidth - e.clientWidth), { message: `${w} table` }).toBe(0);
      |                                                                                                                       ^ Error: 1024 table
  308 |     await page.goto(`/live?tail=1`);
  309 |     await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  310 |     expect(await overflowX(page), `${w} tail`).toBe(0);
  311 |     await page.goto(`/live?clients=1`);
  312 |     await expect(page.getByTestId("clients-sheet")).toBeVisible();
  313 |     expect(await overflowX(page), `${w} sheet`).toBe(0);
  314 |   }
  315 | });
  316 | 
  317 | test.describe("a broker that goes away and comes back", () => {
  318 |   const ports = { kafka: "127.0.0.1:39192", console: "127.0.0.1:39194" };
  319 |   let child: ChildProcess | undefined;
  320 |   let data = "";
  321 |   const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../../target/release/doze-kafka");
  322 |   const start = async () => {
  323 |     child = spawn(bin, ["--listen", ports.kafka, "--data-dir", data, "--console-addr", ports.console], { stdio: "ignore" });
  324 |     const end = Date.now() + 15_000;
  325 |     for (;;) {
  326 |       try {
  327 |         if ((await fetch(`http://${ports.console}/healthz`)).ok) return;
  328 |       } catch {
  329 |         // not yet
  330 |       }
  331 |       if (Date.now() > end) throw new Error("the second broker did not start");
  332 |       await sleep(100);
  333 |     }
  334 |   };
  335 |   const kill = async () => {
  336 |     const c = child;
  337 |     child = undefined;
  338 |     if (!c || c.exitCode !== null) return;
  339 |     const gone = new Promise((r) => c.once("exit", r));
  340 |     c.kill("SIGKILL");
  341 |     await gone;
  342 |   };
  343 |   test.afterEach(async () => {
  344 |     await kill();
  345 |     if (data) rmSync(data, { recursive: true, force: true });
  346 |   });
  347 | 
  348 |   test("the tail says Reconnecting, then Live, with no row twice and none of the old life", async ({ page, errors }) => {
  349 |     test.setTimeout(120_000);
  350 |     data = mkdtempSync(join(tmpdir(), "doze-e2e-r3-"));
  351 |     await start();
  352 |     let stop = traffic(ports.kafka);
  353 |     try {
  354 |       await page.goto(`http://${ports.console}/live`);
  355 |       await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
  356 |       await page.goto(`http://${ports.console}/live?tail=1`);
  357 |       const state = page.getByTestId("tail-state");
  358 |       await expect(state).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  359 |       await page.mouse.move(2, 600);
  360 |       await expect(rowsOf(page).first()).toBeVisible({ timeout: 15_000 });
  361 |       const old = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  362 |       await kill();
  363 |       await expect(state).toHaveAttribute("data-state", /reconnecting|disconnected/, { timeout: 10_000 });
  364 |       await expect(state).toHaveText(/Reconnecting…|Disconnected/);
  365 |       await stop();
  366 |       await start();
  367 |       stop = traffic(ports.kafka);
  368 |       await expect(state).toHaveAttribute("data-state", "live", { timeout: 30_000 });
  369 |       await expect(state).toHaveText("Live");
  370 |       // The new life's rows arrive; none of the old life stays (its ids could be other requests now), none twice.
  371 |       await expect.poll(async () => (await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.textContent ?? ""))).some((t) => t.includes("e2e-")), { timeout: 20_000 }).toBe(true);
  372 |       const ids = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  373 |       expect(new Set(ids).size).toBe(ids.length);
  374 |       expect(old.length).toBeGreaterThan(0);
  375 |     } finally {
  376 |       await stop();
  377 |       // The browser's own report of the requests the test made fail (the broker was killed), not a fault of the page.
  378 |       for (let i = errors.length - 1; i >= 0; i--) if (/ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_INCOMPLETE_CHUNKED|Failed to load resource|NetworkError|network connection was lost|Could not connect/i.test(errors[i]!)) errors.splice(i, 1);
  379 |     }
  380 |   });
  381 | });
  382 | 
```