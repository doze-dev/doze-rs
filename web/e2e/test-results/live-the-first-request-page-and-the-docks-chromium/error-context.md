# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: live.spec.ts >> the first-request page and the docks
- Location: tests/live.spec.ts:324:1

# Error details

```
Error: expect(received).toContain(expected) // indexOf

Expected substring: "client.id='x; open -a Calculator #'"
Received string:    "echo hi | kcat -P -b 127.0.0.1:39092 -t orders -X client.id=lab-once-chromium"

Call Log:
- Timeout 8000ms exceeded while waiting on the predicate
```

# Test source

```ts
  241 |   await expect(page.getByRole("combobox", { name: "Topics" })).toHaveText("orders");
  242 |   await page.getByTestId("refused-only").click();
  243 |   await expect(page).not.toHaveURL(/refused=/);
  244 |   // Idle polls: shown when switched off.
  245 |   await expect(page.getByTestId("pager")).toContainText("idle polls hidden");
  246 |   await page.getByTestId("idle-hidden").click();
  247 |   await expect(page).toHaveURL(/idle=/);
  248 |   await expect(page.getByTestId("pager")).not.toContainText("idle polls hidden");
  249 |   // A search that matches nothing says which filters are on and offers the way out.
  250 |   await page.getByTestId("live-search").fill("no-such-thing-anywhere");
  251 |   await expect(page).toHaveURL(/q=no-such-thing-anywhere/);
  252 |   await expect(page.getByTestId("live-no-match")).toContainText("topic is orders");
  253 |   await page.getByTestId("live-no-match").getByRole("button", { name: "Clear filters" }).click();
  254 |   await expect(page).not.toHaveURL(/topic=|q=/);
  255 |   await expect(rowsOf(page).first()).toBeVisible();
  256 |   // A click on a client in a row filters by it.
  257 |   const client = (await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().innerText()).trim();
  258 |   await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().click();
  259 |   await expect(page).toHaveURL(new RegExp(`client=`));
  260 |   await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()).every((t) => t.includes(client))).toBe(true);
  261 |   await page.goBack();
  262 |   await expect(page).not.toHaveURL(/client=/);
  263 | });
  264 | 
  265 | test("a request opens in a Sheet with its own address: Esc closes, Back closes, a deep link opens it", async ({ page }) => {
  266 |   await openLive(page);
  267 |   const first = rowsOf(page).first();
  268 |   const id = await first.getAttribute("data-row");
  269 |   await first.locator("td").nth(4).click();
  270 |   await expect(page).toHaveURL(new RegExp(`request=${id}`));
  271 |   const sheet = page.getByTestId("request-sheet");
  272 |   await expect(sheet).toBeVisible();
  273 |   await expect(sheet.getByTestId("request-body")).toContainText(/Correlation id|Kafka's words/);
  274 |   await page.keyboard.press("Escape");
  275 |   await expect(sheet).toBeHidden();
  276 |   await expect(page).not.toHaveURL(/request=/);
  277 |   await first.locator("td").nth(4).click();
  278 |   await expect(sheet).toBeVisible();
  279 |   await page.goBack();
  280 |   await expect(sheet).toBeHidden();
  281 |   await expect(page).not.toHaveURL(/request=/);
  282 |   await page.goto(`/live?request=${id}`);
  283 |   await expect(sheet).toBeVisible();
  284 |   await sheet.getByRole("button", { name: "Close" }).click();
  285 |   await expect(sheet).toBeHidden();
  286 |   await expect(page).toHaveURL(/\/live$/);
  287 |   // A request that is not on the tape says so.
  288 |   await page.goto("/live?request=987654321");
  289 |   await expect(sheet.getByTestId("request-gone")).toContainText("no longer on the wire");
  290 | });
  291 | 
  292 | test("a refused request: Kafka's words, the rule behind them, what would pass", async ({ page }) => {
  293 |   await openLive(page, "?refused=1");
  294 |   const row = rowsOf(page).filter({ hasText: "INVALID_RECORD" }).first();
  295 |   await expect(row).toBeVisible();
  296 |   await row.locator("td").nth(4).click();
  297 |   const sheet = page.getByTestId("request-sheet");
  298 |   await expect(sheet).toHaveAttribute("data-kind", "refused");
  299 |   await expect(sheet.getByRole("heading", { name: "INVALID_RECORD (87)" })).toBeVisible();
  300 |   await expect(sheet.getByTestId("kafka-words")).toContainText("Kafka's words");
  301 |   await expect(sheet.getByTestId("rules")).toContainText("cleanup.policy=compact");
  302 |   await expect(sheet.getByText("Give it a key")).toBeVisible();
  303 |   // The refusals strip opens to one line per (code, client).
  304 |   await page.keyboard.press("Escape");
  305 |   await page.getByTestId("refusals").getByRole("button").first().click();
  306 |   await expect(page.getByTestId("refusal-lines")).toContainText("INVALID_RECORD (87)");
  307 | });
  308 | 
  309 | test("the clients Sheet: who is connected, and a name filters the tape", async ({ page }) => {
  310 |   await openLive(page);
  311 |   await page.getByTestId("open-clients").click();
  312 |   await expect(page).toHaveURL(/clients=/);
  313 |   const sheet = page.getByTestId("clients-sheet");
  314 |   await expect(sheet.getByTestId("clients")).toContainText("e2e-orders");
  315 |   await page.keyboard.press("Escape");
  316 |   await expect(page).not.toHaveURL(/clients=/);
  317 |   await page.getByTestId("open-clients").click();
  318 |   await sheet.getByRole("link", { name: "e2e-orders" }).click();
  319 |   await expect(sheet).toBeHidden();
  320 |   await expect(page).toHaveURL(/client=/);
  321 |   await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()).every((t) => t.includes("e2e-orders"))).toBe(true);
  322 | });
  323 | 
  324 | test("the first-request page and the docks", async ({ page }) => {
  325 |   await page.goto("/connect/first-request");
  326 |   await expect(page.locator("h1")).toHaveText("A new client just connected");
  327 |   await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText(/Home.*Connect/);
  328 |   await expect(page.getByTestId("first-banner")).toBeVisible();
  329 |   await expect(page.getByTestId("first-story").locator("li").first()).toContainText(/ApiVersions|Metadata/);
  330 |   // A client whose id a shell would act on: the command the page offers quotes it (the server makes it with shell_word).
  331 |   const hostile = "x; open -a Calculator #";
  332 |   const admin = new Kafka({ clientId: hostile, brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 0 } }).admin();
  333 |   await admin.connect();
  334 |   await admin.listTopics();
  335 |   await admin.disconnect();
  336 |   await expect
  337 |     .poll(async () => {
  338 |       await page.reload();
  339 |       return page.getByTestId("first-request").locator("[data-slot=code-value] code").last().textContent();
  340 |     })
> 341 |     .toContain(`client.id='${hostile}'`);
      |      ^ Error: expect(received).toContain(expected) // indexOf
  342 |   const command = await page.getByTestId("first-request").locator("[data-slot=code-value] code").last().textContent();
  343 |   expect(command).not.toContain(`client.id=${hostile}`);
  344 |   await page.goto("/gallery");
  345 |   const dock = page.locator("[data-testid=wire-dock][data-topic=orders]");
  346 |   await expect(dock.locator("tbody tr[data-row]").first()).toBeVisible();
  347 |   for (const t of await dock.locator("tbody tr[data-row] td:nth-child(4)").allInnerTexts()) expect(t).toContain("orders");
  348 |   const group = page.locator("[data-testid=wire-dock][data-group=fulfilment]");
  349 |   await group.getByRole("radio", { name: "Commits" }).click();
  350 |   await expect(group.getByRole("radio", { name: "Commits" })).toHaveAttribute("aria-checked", "true");
  351 |   await dock.locator("tbody tr[data-row]").first().getByTestId("row-time").click();
  352 |   await expect(page).toHaveURL(/\/live\?request=\d+/);
  353 |   await expect(page.getByTestId("request-sheet")).toBeVisible();
  354 | });
  355 | 
  356 | test("no page scrolls sideways at 1024 to 1920, in a snapshot, in the tail and under a Sheet", async ({ page }) => {
  357 |   for (const w of [1024, 1280, 1440, 1920]) {
  358 |     await page.setViewportSize({ width: w, height: 900 });
  359 |     await openLive(page);
  360 |     expect(await overflowX(page), `${w} snapshot`).toBe(0);
  361 |     await expect.poll(() => scroller(page).evaluate((e) => e.scrollWidth - e.clientWidth), { message: `${w} table` }).toBe(0);
  362 |     if (w >= 1280) expect(await cutHeaders(page), `headers cut at ${w}`).toEqual([]);
  363 |     await page.goto(`/live?tail=1`);
  364 |     await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  365 |     expect(await overflowX(page), `${w} tail`).toBe(0);
  366 |     await page.goto(`/live?clients=1`);
  367 |     await expect(page.getByTestId("clients-sheet")).toBeVisible();
  368 |     expect(await overflowX(page), `${w} sheet`).toBe(0);
  369 |   }
  370 | });
  371 | 
  372 | test.describe("a broker that goes away and comes back", () => {
  373 |   const ports = { kafka: "127.0.0.1:39192", console: "127.0.0.1:39194" };
  374 |   let child: ChildProcess | undefined;
  375 |   let data = "";
  376 |   const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../../target/release/doze-kafka");
  377 |   const start = async () => {
  378 |     child = spawn(bin, ["--listen", ports.kafka, "--data-dir", data, "--console-addr", ports.console], { stdio: "ignore" });
  379 |     const end = Date.now() + 15_000;
  380 |     for (;;) {
  381 |       try {
  382 |         if ((await fetch(`http://${ports.console}/healthz`)).ok) return;
  383 |       } catch {
  384 |         // not yet
  385 |       }
  386 |       if (Date.now() > end) throw new Error("the second broker did not start");
  387 |       await sleep(100);
  388 |     }
  389 |   };
  390 |   const kill = async () => {
  391 |     const c = child;
  392 |     child = undefined;
  393 |     if (!c || c.exitCode !== null) return;
  394 |     const gone = new Promise((r) => c.once("exit", r));
  395 |     c.kill("SIGKILL");
  396 |     await gone;
  397 |   };
  398 |   test.afterEach(async () => {
  399 |     await kill();
  400 |     if (data) rmSync(data, { recursive: true, force: true });
  401 |   });
  402 | 
  403 |   test("the tail says Reconnecting, then Live, with no row twice and none of the old life", async ({ page, errors }) => {
  404 |     test.setTimeout(120_000);
  405 |     data = mkdtempSync(join(tmpdir(), "doze-e2e-r3-"));
  406 |     await start();
  407 |     let stop = traffic(ports.kafka);
  408 |     try {
  409 |       await page.goto(`http://${ports.console}/live`);
  410 |       await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
  411 |       await page.goto(`http://${ports.console}/live?tail=1`);
  412 |       const state = page.getByTestId("tail-state");
  413 |       await expect(state).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  414 |       await page.mouse.move(2, 600);
  415 |       await expect(rowsOf(page).first()).toBeVisible({ timeout: 15_000 });
  416 |       const old = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  417 |       await kill();
  418 |       await expect(state).toHaveAttribute("data-state", /reconnecting|disconnected/, { timeout: 10_000 });
  419 |       await expect(state).toHaveText(/Reconnecting…|Disconnected/);
  420 |       await stop();
  421 |       await start();
  422 |       stop = traffic(ports.kafka);
  423 |       await expect(state).toHaveAttribute("data-state", "live", { timeout: 30_000 });
  424 |       await expect(state).toHaveText("Live");
  425 |       // The new life's rows arrive; none of the old life stays (its ids could be other requests now), none twice.
  426 |       await expect.poll(async () => (await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.textContent ?? ""))).some((t) => t.includes("e2e-")), { timeout: 20_000 }).toBe(true);
  427 |       const ids = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  428 |       expect(new Set(ids).size).toBe(ids.length);
  429 |       expect(old.length).toBeGreaterThan(0);
  430 |     } finally {
  431 |       await stop();
  432 |       // The browser's own report of the requests the test made fail (the broker was killed), not a fault of the page.
  433 |       for (let i = errors.length - 1; i >= 0; i--) if (/ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_INCOMPLETE_CHUNKED|Failed to load resource|NetworkError|network connection was lost|Could not connect/i.test(errors[i]!)) errors.splice(i, 1);
  434 |     }
  435 |   });
  436 | });
  437 | 
```