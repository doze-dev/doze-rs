# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: shell.spec.ts >> a deep link opens the page as it was, and the classic console is gone
- Location: tests/shell.spec.ts:138:1

# Error details

```
Error: expect(locator).toHaveCount(expected) failed

Locator:  getByTestId('sidebar').getByTestId('classic-link')
Expected: 0
Received: 1
Timeout:  8000ms

Call log:
  - Expect "toHaveCount" getByTestId('sidebar').getByTestId('classic-link') with timeout 8000ms
  - waiting for getByTestId('sidebar').getByTestId('classic-link')
    20 × locator resolved to 1 element
       - unexpected value "1"

```

# Page snapshot

```yaml
- generic [ref=f3e2]:
  - generic [ref=f3e3]:
    - generic [ref=f3e6]:
      - link "doze Kafka, Home" [ref=f3e8] [cursor=pointer]:
        - /url: /
        - generic [ref=f3e9]:
          - generic [ref=f3e10]: doze Kafka
          - text: doze
          - generic [aria-hidden] [ref=f3e11]: Kafka
      - list [ref=f3e15]:
        - listitem [ref=f3e16]:
          - link "Home" [ref=f3e17] [cursor=pointer]:
            - /url: /
        - listitem [ref=f3e22]:
          - link "Live" [ref=f3e23] [cursor=pointer]:
            - /url: /live
        - listitem [ref=f3e27]:
          - link "Topics" [ref=f3e28] [cursor=pointer]:
            - /url: /topics
          - generic: "4"
        - listitem [ref=f3e32]:
          - link "Groups" [ref=f3e33] [cursor=pointer]:
            - /url: /groups
          - generic: "3"
        - listitem [ref=f3e40]:
          - link "Access" [ref=f3e41] [cursor=pointer]:
            - /url: /access
        - listitem [ref=f3e45]:
          - link "Lab" [ref=f3e46] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=f3e50]:
          - link "Settings" [ref=f3e51] [cursor=pointer]:
            - /url: /settings
      - list [ref=f3e57]:
        - listitem [ref=f3e58]:
          - link "Classic console" [ref=f3e59] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=f3e65]
    - main [ref=f3e66]:
      - generic [ref=f3e67]:
        - button "Toggle Sidebar" [ref=f3e68]
        - button "Jump to a topic or group ⌘K" [ref=f3e70]:
          - generic [ref=f3e74]: Jump to a topic or group
          - generic [ref=f3e75]: ⌘K
        - generic [ref=f3e76]:
          - button "127.0.0.1:39092" [ref=f3e77]
          - generic [ref=f3e82]: Kafka 4 profile
          - link "Open" [ref=f3e83] [cursor=pointer]:
            - /url: /access
          - radiogroup "Theme" [ref=f3e85]:
            - radio "System" [checked] [ref=f3e86]
            - radio "Light" [ref=f3e89]
            - radio "Dark" [ref=f3e96]
      - main [ref=f3e99]:
        - heading "Not here" [level=1] [ref=f3e103]
        - generic [ref=f3e104]:
          - heading "The console has no page at this address" [level=3] [ref=f3e109]
          - generic [ref=f3e110]: Jump to a topic or group with ⌘K, or start from Home.
          - link "Home" [ref=f3e112] [cursor=pointer]:
            - /url: /
      - generic [ref=f3e113]:
        - heading "Jump to a topic or group" [level=2] [ref=f3e114]
        - paragraph [ref=f3e115]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  44  |   await expect(chip).toHaveText(/\d+\.\d+\.\d+\.\d+:\d+/);
  45  |   await chip.click();
  46  |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Address copied" })).toBeVisible();
  47  |   await expect(page.getByTestId("profile-chip")).toHaveText(/Kafka \d+ profile/);
  48  |   await page.getByTestId("security-chip").click();
  49  |   await expect(page).toHaveURL(/\/access$/);
  50  | });
  51  | 
  52  | test("the palette opens with the keyboard, finds a topic by part of its name, goes there and remembers it", async ({ page }) => {
  53  |   await page.keyboard.press("ControlOrMeta+k");
  54  |   const input = page.getByTestId("palette-input");
  55  |   await expect(input).toBeFocused();
  56  |   await input.fill("ord");
  57  |   const item = page.getByRole("option", { name: "orders", exact: true });
  58  |   await expect(item).toBeVisible();
  59  |   await expect(page.getByRole("group", { name: "Topics" })).toBeVisible();
  60  |   await expect(page.getByRole("group", { name: "Pages" })).toHaveCount(0);
  61  |   await item.click();
  62  |   await expect(page).toHaveURL(/\/topics\/orders$/);
  63  |   await expect(page.locator("h1")).toHaveText("orders");
  64  |   await expect(input).toBeHidden();
  65  |   // Recents
  66  |   await page.getByTestId("palette-trigger").click();
  67  |   await expect(page.getByRole("group", { name: "Recents" }).getByRole("option", { name: /orders/ })).toBeVisible();
  68  |   // Esc closes and gives the focus back to the trigger.
  69  |   await page.keyboard.press("Escape");
  70  |   await expect(page.getByTestId("palette-input")).toBeHidden();
  71  |   await expect(page.getByTestId("palette-trigger")).toBeFocused();
  72  | });
  73  | 
  74  | test("the palette goes to a page with the arrow keys and Enter, and says when nothing matches", async ({ page }) => {
  75  |   await page.getByTestId("palette-trigger").click();
  76  |   await page.getByTestId("palette-input").fill("gallery");
  77  |   await page.keyboard.press("ArrowDown");
  78  |   await page.keyboard.press("ArrowUp");
  79  |   await page.keyboard.press("Enter");
  80  |   await expect(page).toHaveURL(/\/gallery$/);
  81  |   await page.keyboard.press("ControlOrMeta+k");
  82  |   await page.getByTestId("palette-input").fill("zzz-nothing-is-called-this");
  83  |   await expect(page.getByTestId("palette-list")).toContainText("Nothing is called");
  84  | });
  85  | 
  86  | test("the refresh interval is chosen with the mouse, remembered across a reload, and Off stops the timer", async ({ page }) => {
  87  |   const trigger = page.getByTestId("refresh-interval");
  88  |   await expect(trigger).toHaveText("Refresh: Off");
  89  |   await choose(page, trigger, "Every 5 s");
  90  |   await expect(trigger).toHaveText("Every 5 s");
  91  |   expect(await page.evaluate(() => localStorage.getItem("doze:refresh"))).toBe("5s");
  92  |   await page.reload();
  93  |   await expect(page.getByTestId("refresh-interval")).toHaveText("Every 5 s");
  94  |   // The interval fires: the stamp moves on, and nothing on the page moves.
  95  |   const stamp = page.getByTestId("refresh-stamp");
  96  |   const first = await stamp.textContent();
  97  |   const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=home-callout]", "[data-testid=address-card]", "[data-testid=instance-card]", "[data-testid=next-card]"];
  98  |   const before = await boxes(page, selectors);
  99  |   await expect(stamp).not.toHaveText(first ?? "", { timeout: 12_000 });
  100 |   const after = await boxes(page, selectors);
  101 |   expect(after).toEqual(before);
  102 |   // With the keyboard this time: Off.
  103 |   await page.getByTestId("refresh-interval").focus();
  104 |   await page.keyboard.press("Enter");
  105 |   await page.getByRole("option", { name: "Refresh: Off" }).press("Enter");
  106 |   await expect(page.getByTestId("refresh-interval")).toHaveText("Refresh: Off");
  107 |   const still = await stamp.textContent();
  108 |   await page.waitForTimeout(6_000);
  109 |   await expect(stamp).toHaveText(still ?? "");
  110 | });
  111 | 
  112 | test("Refresh and the r key fetch the page again", async ({ page }) => {
  113 |   const stamp = page.getByTestId("refresh-stamp");
  114 |   await page.waitForTimeout(1_100);
  115 |   const first = await stamp.textContent();
  116 |   await page.getByTestId("refresh-now").click();
  117 |   await expect(stamp).not.toHaveText(first ?? "");
  118 |   await page.waitForTimeout(1_100);
  119 |   const second = await stamp.textContent();
  120 |   await page.locator("h1").click();
  121 |   await page.keyboard.press("r");
  122 |   await expect(stamp).not.toHaveText(second ?? "");
  123 | });
  124 | 
  125 | for (const width of [1024, 1280, 1440, 1920]) {
  126 |   test(`nothing scrolls sideways at ${width} px`, async ({ page }) => {
  127 |     await page.setViewportSize({ width, height: 900 });
  128 |     for (const path of ["/", "/connect", "/gallery", "/gallery?rows=1000", "/topics", "/topics/a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2"]) {
  129 |       await page.goto(path);
  130 |       await expect(page.locator("h1")).toBeVisible();
  131 |       await page.waitForLoadState("networkidle");
  132 |       expect(await overflowX(page), `${path} at ${width}`).toBeLessThanOrEqual(0);
  133 |       if (width >= 1280) expect(await cutHeaders(page), `headers cut on ${path} at ${width}`).toEqual([]);
  134 |     }
  135 |   });
  136 | }
  137 | 
  138 | test("a deep link opens the page as it was, and the classic console is gone", async ({ page }) => {
  139 |   await page.goto("/connect?as=environment");
  140 |   await expect(page.getByTestId("copy-as").getByRole("radio", { name: "Environment" })).toHaveAttribute("aria-checked", "true");
  141 |   await page.goto("/no/such/page");
  142 |   await expect(page.locator("h1")).toHaveText("Not here");
  143 |   // The server-rendered console is gone (slice R5): no link to it, and its old addresses are this app's Not here.
> 144 |   await expect(page.getByTestId("sidebar").getByTestId("classic-link")).toHaveCount(0);
      |                                                                         ^ Error: expect(locator).toHaveCount(expected) failed
  145 |   await expect(page.getByTestId("sidebar")).not.toContainText(/classic/i);
  146 |   for (const path of ["/classic/", "/classic/topics/orders"]) {
  147 |     const res = await page.goto(path);
  148 |     expect(res?.status(), path).toBe(200);
  149 |     await expect(page.locator("h1")).toHaveText("Not here");
  150 |   }
  151 |   const api = await page.request.get("/api/v1/shell", { headers: { "X-Console": "1" } });
  152 |   expect(Object.keys(await api.json())).not.toContain("classic");
  153 | });
  154 | 
```