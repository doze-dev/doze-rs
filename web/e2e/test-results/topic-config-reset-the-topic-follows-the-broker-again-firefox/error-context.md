# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-config.spec.ts >> reset: the topic follows the broker again
- Location: tests/topic-config.spec.ts:137:1

# Error details

```
Error: expect(locator).toContainText(expected) failed

Locator: getByTestId('dialog-config-reset').getByTestId('reset-after')
Expected substring: "604800000"
Received string:    "-1 no time limit · Static flag"
Timeout: 8000ms

Call log:
  - Expect "toContainText" getByTestId('dialog-config-reset').getByTestId('reset-after') with timeout 8000ms
  - waiting for getByTestId('dialog-config-reset').getByTestId('reset-after')
    20 × locator resolved to <dd data-testid="reset-after">…</dd>
       - unexpected value "-1 no time limit · Static flag"

```

```yaml
- definition:
  - code: "-1"
  - text: no time limit · Static flag
```

# Test source

```ts
  47  |   const y = await page.evaluate(() => window.scrollY);
  48  |   await r.getByRole("button", { name: "Kafka's text" }).click();
  49  |   await expect(r.getByTestId("kafka-text")).toBeVisible();
  50  |   await expect(r.getByTestId("kafka-text")).toContainText("Type");
  51  |   expect(page.url()).toBe(url);
  52  |   expect(Math.abs((await page.evaluate(() => window.scrollY)) - y)).toBeLessThan(2);
  53  |   await r.getByRole("button", { name: "Kafka's text" }).click();
  54  |   await expect(r.getByTestId("kafka-text")).toBeHidden();
  55  |   const ret = row(page, "retention.ms");
  56  |   await ret.getByRole("button", { name: "Where from" }).click();
  57  |   await expect(ret.getByTestId("where-from")).toContainText("Dynamic topic");
  58  |   await expect(ret.getByTestId("where-from")).toContainText("takes over");
  59  |   expect(page.url()).toBe(url);
  60  | });
  61  | 
  62  | test("edit a value: Check asks Kafka and changes nothing, Save changes it, and the tab shows it without a reload", async ({ page, request, baseURL }, info) => {
  63  |   const name = fresh(info, "cfge");
  64  |   await topic(request, baseURL!, name, 1, "short");
  65  |   await page.goto(configUrl(name));
  66  |   const r = row(page, "retention.ms");
  67  |   await r.getByTestId("row-menu").click();
  68  |   await page.getByRole("menuitem", { name: "Edit" }).click();
  69  |   await expect(page).toHaveURL(/edit=retention\.ms/);
  70  |   const d = page.getByTestId("dialog-config-edit");
  71  |   await expect(d.getByTestId("config-value")).toHaveValue("3600000");
  72  |   await d.getByTestId("config-value").fill("7200000");
  73  |   await expect(d.getByTestId("the-call")).toContainText("7200000");
  74  |   await d.getByTestId("config-check").click();
  75  |   await expect(d.getByTestId("config-checked")).toContainText("Kafka would take this");
  76  |   const still = await get(request, `/api/v1/topics/${name}/config`);
  77  |   expect(still.body.rows.find((x: { name: string }) => x.name === "retention.ms").value).toBe("3600000");
  78  |   await d.getByTestId("config-save").click();
  79  |   await expect(d.getByTestId("config-saved")).toBeVisible();
  80  |   await expect(d.getByTestId("saved-now")).toContainText("7200000");
  81  |   await d.getByTestId("saved-done").click();
  82  |   await expect(d).toBeHidden();
  83  |   await expect(page).not.toHaveURL(/edit=/);
  84  |   await expect(r.getByTestId("config-value")).toHaveText("7200000");
  85  | });
  86  | 
  87  | test("a list: append a policy", async ({ page, request, baseURL }, info) => {
  88  |   const name = fresh(info, "cfgl");
  89  |   await topic(request, baseURL!, name, 1, "event");
  90  |   await page.goto(configUrl(name, "?edit=cleanup.policy"));
  91  |   const d = page.getByTestId("dialog-config-edit");
  92  |   await expect(d.getByTestId("config-op").getByRole("radio", { name: /Append/ })).toHaveAttribute("aria-checked", "true");
  93  |   await d.getByTestId("config-value").fill("compact");
  94  |   await expect(d.getByTestId("the-call")).toContainText("APPEND");
  95  |   await d.getByTestId("config-save").click();
  96  |   await expect(d.getByTestId("saved-now")).toContainText("delete,compact");
  97  |   await page.keyboard.press("Escape");
  98  |   await expect(row(page, "cleanup.policy").getByTestId("config-value")).toHaveText("delete,compact");
  99  | });
  100 | 
  101 | test("the two lags together: min above max is refused in Kafka's words, Edit again, then saved", async ({ page, request, baseURL }, info) => {
  102 |   const name = fresh(info, "cfgg");
  103 |   await topic(request, baseURL!, name, 1, "compact");
  104 |   await page.goto(configUrl(name, "?edit=min.compaction.lag.ms"));
  105 |   const d = page.getByTestId("dialog-config-edit");
  106 |   await expect(d.getByRole("heading", { name: "Edit compaction lag" })).toBeVisible();
  107 |   await d.getByTestId("config-value").fill("50000");
  108 |   await d.getByTestId("config-other").fill("10000");
  109 |   await d.getByTestId("config-save").click();
  110 |   const refused = d.getByTestId("config-refused");
  111 |   await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG");
  112 |   const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.compaction.lag.ms", op: "set", value: "50000", other: "10000" });
  113 |   await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  114 |   await expect(refused.getByTestId("would-pass")).toBeVisible();
  115 |   await refused.getByTestId("edit-again").click();
  116 |   await d.getByTestId("config-value").fill("1000");
  117 |   await d.getByTestId("config-other").fill("60000");
  118 |   await d.getByTestId("config-save").click();
  119 |   await expect(d.getByTestId("saved-change")).toHaveCount(2);
  120 | });
  121 | 
  122 | test("a value Kafka will not take: refused in its own words, and nothing changed", async ({ page, request, baseURL }, info) => {
  123 |   const name = fresh(info, "cfgv");
  124 |   await topic(request, baseURL!, name, 1, "event");
  125 |   await page.goto(configUrl(name, "?edit=min.cleanable.dirty.ratio"));
  126 |   const d = page.getByTestId("dialog-config-edit");
  127 |   await d.getByTestId("config-value").fill("7");
  128 |   await d.getByTestId("config-check").click();
  129 |   const refused = d.getByTestId("config-refused");
  130 |   await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG (40)");
  131 |   const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.cleanable.dirty.ratio", op: "set", value: "7", other: "", check: "1" });
  132 |   await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  133 |   const still = await get(request, `/api/v1/topics/${name}/config`);
  134 |   expect(still.body.rows.find((x: { name: string }) => x.name === "min.cleanable.dirty.ratio").source).not.toBe("dynamic_topic");
  135 | });
  136 | 
  137 | test("reset: the topic follows the broker again", async ({ page, request, baseURL }, info) => {
  138 |   const name = fresh(info, "cfgr");
  139 |   await topic(request, baseURL!, name, 1, "short");
  140 |   await page.goto(configUrl(name));
  141 |   const r = row(page, "retention.ms");
  142 |   await r.getByTestId("row-menu").click();
  143 |   await page.getByRole("menuitem", { name: "Reset to the inherited value" }).click();
  144 |   await expect(page).toHaveURL(/reset=retention\.ms/);
  145 |   const d = page.getByTestId("dialog-config-reset");
  146 |   await expect(d.getByTestId("reset-now")).toContainText("3600000");
> 147 |   await expect(d.getByTestId("reset-after")).toContainText("604800000");
      |                                              ^ Error: expect(locator).toContainText(expected) failed
  148 |   await d.getByTestId("reset-submit").click();
  149 |   await expect(d).toBeHidden();
  150 |   await expect(page.locator("[data-sonner-toast]").filter({ hasText: "Reset retention.ms." })).toBeVisible();
  151 |   await expect(row(page, "retention.ms").getByTestId("source")).toHaveText("Default");
  152 |   await expect(row(page, "retention.ms").getByTestId("config-value")).toHaveText("604800000");
  153 | });
  154 | 
```