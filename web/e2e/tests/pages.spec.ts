import { expect, test } from "./fixtures";

test("Home: one h1, the figures as stat tiles, no memory figure anywhere, the address of the broker", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("h1")).toHaveCount(1);
  const strip = page.locator("[data-slot=stat-strip]");
  for (const label of ["Started", "Topics", "Open partitions", "Apps on the wire", "Last request"]) await expect(strip).toContainText(label);
  const text = (await page.locator("body").innerText()).toLowerCase();
  for (const word of ["memory", "resident", " mib", " kib"]) expect(text, word).not.toContain(word);
  const address = await page.getByTestId("address-chip").innerText();
  await expect(page.getByTestId("address-card").locator("[data-slot=code-value] code")).toHaveText(address.trim());
  await expect(page.getByTestId("home-callout")).toBeVisible();
  await expect(page.getByTestId("instance-card")).toContainText("Running");
});

test("Home: Check again runs the advertised-listener check and says what it found", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("probe-rules").locator("li")).toHaveCount(3);
  await page.getByTestId("check-again").click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Checked the advertised listener" })).toBeVisible();
  await expect(page.getByTestId("address-card").getByTestId("verdict")).toHaveText(/Reachable|Not reachable|Not checked/);
});

test("Home's links go where they say", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("link", { name: "Snippets for every client" }).click();
  await expect(page).toHaveURL(/\/connect$/);
  await page.goBack();
  await page.getByTestId("instance-card").getByRole("link", { name: /Settings/ }).click();
  await expect(page).toHaveURL(/\/settings$/);
  // Settings is rebuilt (slice R4): the page itself, not the classic one.
  await expect(page.locator("h1")).toHaveText("Settings");
});

test("Connect: Copy as changes what Copy copies, is in the address, and Back restores it", async ({ page }) => {
  await page.goto("/connect");
  await expect(page.locator("h1")).toHaveText("Connect an app");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Home");
  const snippets = page.getByTestId("snippet");
  await expect(snippets.first()).toBeVisible();
  expect(await snippets.count()).toBeGreaterThanOrEqual(4);
  const group = page.getByTestId("copy-as");
  await group.getByRole("radio", { name: "Properties" }).click();
  await expect(page).toHaveURL(/as=properties/);
  await snippets.first().getByRole("button", { name: "Copy" }).click();
  await expect(page.locator("[data-sonner-toast]").filter({ hasText: "copied as properties" })).toBeVisible();
  await expect(snippets.first().getByRole("button", { name: "Copied" })).toBeVisible();
  await group.getByRole("radio", { name: "Environment" }).click();
  await expect(page).toHaveURL(/as=environment/);
  await page.goBack();
  await expect(page).toHaveURL(/as=properties/);
  await expect(group.getByRole("radio", { name: "Properties" })).toHaveAttribute("aria-checked", "true");
  // The keyboard moves the choice too (roving focus, arrows).
  await group.getByRole("radio", { name: "Properties" }).focus();
  await page.keyboard.press("ArrowLeft");
  await expect(group.getByRole("radio", { name: "Code" })).toBeFocused();
  await page.keyboard.press("Space");
  await expect(group.getByRole("radio", { name: "Code" })).toHaveAttribute("aria-checked", "true");
  await expect(page).not.toHaveURL(/as=/);
});

test("Connect: the address card and the advertised listener card stand side by side at 1440 and stack at 1024", async ({ page }) => {
  await page.goto("/connect");
  const cards = page.getByTestId("connect").locator("> div").first().locator("> [data-slot=card]");
  await expect(cards).toHaveCount(2);
  const [a, b] = [await cards.nth(0).boundingBox(), await cards.nth(1).boundingBox()];
  expect(Math.round(a!.y)).toBe(Math.round(b!.y));
  await page.setViewportSize({ width: 1024, height: 800 });
  const [c, d] = [await cards.nth(0).boundingBox(), await cards.nth(1).boundingBox()];
  expect(d!.y).toBeGreaterThan(c!.y + c!.height - 1);
});

test("an API request without the console's header is refused in JSON", async ({ request }) => {
  const res = await request.get("/api/v1/home");
  expect(res.status()).toBe(403);
  expect((await res.json()).code).toBe("FORBIDDEN");
  const ok = await request.get("/api/v1/shell", { headers: { "X-Console": "1" } });
  expect(ok.status()).toBe(200);
  const csp = ok.headers()["content-security-policy"];
  expect(csp).toContain("default-src 'self'");
  expect(csp).not.toContain("unsafe-eval");
});
