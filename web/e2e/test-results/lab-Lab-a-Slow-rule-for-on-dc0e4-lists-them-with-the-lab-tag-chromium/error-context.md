# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: lab.spec.ts >> Lab: a Slow rule for one client slows its requests; the rule counts them; the sheet lists them with the lab tag
- Location: tests/lab.spec.ts:27:1

# Error details

```
Error: expect(received).toBeGreaterThanOrEqual(expected)

Expected: >= 300
Received:    0
```

# Page snapshot

```yaml
- generic:
  - generic:
    - generic [aria-hidden]:
      - generic:
        - generic:
          - generic:
            - generic:
              - link:
                - /url: /
                - generic: doze-kafka
            - generic:
              - generic:
                - generic:
                  - list:
                    - listitem:
                      - link:
                        - /url: /
                        - generic: Home
                    - listitem:
                      - link:
                        - /url: /live
                        - generic: Live
                    - listitem:
                      - link:
                        - /url: /topics
                        - generic: Topics
                      - generic: "3"
                    - listitem:
                      - link:
                        - /url: /groups
                        - generic: Groups
                      - generic: "1"
                    - listitem:
                      - link:
                        - /url: /access
                        - generic: Access
                    - listitem:
                      - link:
                        - /url: /lab
                        - generic: Lab
                    - listitem:
                      - link:
                        - /url: /settings
                        - generic: Settings
            - generic:
              - list:
                - listitem:
                  - link:
                    - /url: /classic/
                    - generic: Classic console
            - button
      - main:
        - generic:
          - button:
            - generic: Toggle Sidebar
          - button:
            - generic: Jump to a topic or group
            - generic: ⌘K
          - generic:
            - button: 127.0.0.1:39092
            - generic: Kafka 4 profile
            - link:
              - /url: /access
              - text: Open
        - main:
          - generic:
            - generic:
              - generic:
                - generic: Updated 05:30:29 UTC
                - combobox:
                  - generic: "Refresh: Off"
                - button: Refresh
            - generic:
              - generic:
                - heading [level=1]: Lab
                - paragraph: Rehearse a failure on purpose and watch what your app does. Each rehearsal is a visible rule with an end time, applied where connections are served, and every request it touches shows on the wire with a lab tag.
              - generic:
                - button: New rehearsal
          - generic:
            - generic:
              - generic:
                - term: Rules in force
                - generic:
                  - definition: 1 of 50
                - generic: 1 switched on
              - generic:
                - term: Hits
                - generic:
                  - definition: "3"
                - generic: by the rules in force
              - generic:
                - term: Ended
                - generic:
                  - definition: "0"
                - generic: in the last hour
              - generic:
                - term: Done once
                - generic:
                  - definition: "0"
                - generic: drops, expiries, rebalances, cleans
            - region:
              - generic:
                - generic:
                  - heading [level=2]: Active
                  - paragraph: What is bending your broker right now. Every rule ends by itself; removing it ends it sooner. A rule switched off matches nothing until switched on.
              - generic:
                - table:
                  - rowgroup:
                    - row:
                      - columnheader: "On"
                      - columnheader:
                        - button:
                          - generic: Rule
                      - columnheader:
                        - button:
                          - generic: Kind
                      - columnheader:
                        - button:
                          - generic: Applies to
                      - columnheader:
                        - button:
                          - generic: Share
                      - columnheader:
                        - button:
                          - generic: Hits
                      - columnheader:
                        - button:
                          - generic: Last hit
                      - columnheader:
                        - button:
                          - generic: Ends
                      - columnheader:
                        - generic: Remove
                  - rowgroup:
                    - row:
                      - cell:
                        - generic:
                          - switch [checked]
                      - cell:
                        - button: Slow produce to orders
                      - cell:
                        - generic: slow
                      - cell: Produce · orders
                      - cell: every request
                      - cell: "3"
                      - cell: 05:30:29
                      - cell: 05:35
                      - cell:
                        - button
            - region:
              - generic:
                - generic:
                  - heading [level=2]: Start a rehearsal
                  - paragraph: Only what production code can honestly cause. Slow and Fail are rules that end on their own; the others happen once and leave a line of what they did.
              - generic:
                - button:
                  - generic:
                    - generic: Slow requests
                    - generic: rule
                  - paragraph: Delay the answer to the requests you choose, for a while. Your producers and consumers see slow, not broken.
                - button:
                  - generic:
                    - generic: Fail some requests
                    - generic: rule
                  - paragraph: Answer 1 in N of the requests you choose with an error Kafka has, such as REQUEST_TIMED_OUT, without asking the broker.
                - button:
                  - generic:
                    - generic: Drop a connection
                    - generic: once
                  - paragraph: Close one app's connections and see whether it reconnects on its own; or keep closing them for a few minutes.
                - button:
                  - generic:
                    - generic: Expire a member's session
                    - generic: once
                  - paragraph: Remove a group member as a missed heartbeat would, and read the story it leaves.
                - button:
                  - generic:
                    - generic: Force a rebalance
                    - generic: once
                  - paragraph: Start a rebalance of one group without waiting for a reason.
                - button:
                  - generic:
                    - generic: Run the cleaner now
                    - generic: once
                  - paragraph: Clean one compacted partition this minute instead of at the cleaner's next look.
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Slow produce to orders Active" [active] [ref=f1e2]:
    - generic [ref=f1e3]:
      - heading [level=2] [ref=f1e4]:
        - text: Slow produce to orders
        - generic [ref=f1e5]: Active
      - paragraph [ref=f1e7]: +300 ms on every Produce request to orders from lab-slow-chromium.
    - generic [ref=f1e8]:
      - generic [ref=f1e9]:
        - generic [ref=f1e10]:
          - switch "Rule 3 on" [checked] [ref=f1e11]
          - text: "On: it fires on the requests it matches"
        - button "Remove rule" [ref=f1e12]
      - generic [ref=f1e13]:
        - generic [ref=f1e14]:
          - term [ref=f1e15]: Hits
          - definition [ref=f1e17]: "3"
          - generic "of 3 matched · every request" [ref=f1e18]
        - generic [ref=f1e19]:
          - term [ref=f1e20]: Last hit
          - definition [ref=f1e22]: 05:30:29
          - generic "UTC" [ref=f1e23]
        - generic [ref=f1e24]:
          - term [ref=f1e25]: Time left
          - definition [ref=f1e26]:
            - time [ref=f1e27]: 4:59
          - generic [ref=f1e28]: ends 05:35 UTC
        - generic [ref=f1e29]:
          - term [ref=f1e30]: Added delay
          - definition [ref=f1e32]: "300"
          - generic "ms each" [ref=f1e33]
      - generic [ref=f1e34]:
        - heading "The last hits" [level=3] [ref=f1e35]
        - paragraph [ref=f1e36]: The same rows as on Live, only the ones this rule touched.
        - table "Requests the rule touched" [ref=f1e38]:
          - rowgroup [ref=f1e47]:
            - row [ref=f1e48]:
              - columnheader "Time" [ref=f1e49]:
                - button "Time" [ref=f1e50]
              - columnheader "Client" [ref=f1e53]:
                - button "Client" [ref=f1e54]
              - columnheader "Request" [ref=f1e57]:
                - button "Request" [ref=f1e58]
              - columnheader "Target" [ref=f1e61]:
                - button "Target" [ref=f1e62]
              - columnheader "Result" [ref=f1e65]:
                - button "Result" [ref=f1e66]
              - columnheader "Took" [ref=f1e69]:
                - button "Took" [ref=f1e70]
              - columnheader "Lab" [ref=f1e73]
          - rowgroup [ref=f1e75]:
            - row [ref=f1e76]:
              - cell "05:30:28" [ref=f1e77]
              - cell "lab-slow-chromium" [ref=f1e78]
              - cell "Produce v7" [ref=f1e79]
              - cell "orders-1" [ref=f1e80]
              - cell [ref=f1e81]:
                - generic "1 record" [ref=f1e82]: Done
              - cell "302 ms" [ref=f1e85]
              - cell [ref=f1e86]:
                - 'link "Lab: Slow produce to orders, made the answer wait 300 ms more" [ref=f1e87] [cursor=pointer]':
                  - /url: /lab?rule=3
                  - text: lab
            - row [ref=f1e90]:
              - cell "05:30:28" [ref=f1e91]
              - cell "lab-slow-chromium" [ref=f1e92]
              - cell "Produce v7" [ref=f1e93]
              - cell "orders-1" [ref=f1e94]
              - cell [ref=f1e95]:
                - generic "1 record" [ref=f1e96]: Done
              - cell "301 ms" [ref=f1e99]
              - cell [ref=f1e100]:
                - 'link "Lab: Slow produce to orders, made the answer wait 300 ms more" [ref=f1e101] [cursor=pointer]':
                  - /url: /lab?rule=3
                  - text: lab
      - paragraph [ref=f1e104]: "The first hit: lab-slow-chromium, Produce v7 to orders-1 at 05:30:28 UTC."
      - paragraph [ref=f1e105]: "Apps it touched: lab-slow-chromium."
      - paragraph [ref=f1e106]: Nothing is wrong with the broker. The rule only changes the answer, so no record changes and no offset moves. When it ends, requests behave as before with no restart.
    - button "Close" [ref=f1e107]
```

# Test source

```ts
  1   | // The Lab (slice R4) against the real binary with traffic: a Slow rule for one client slows its requests and counts them; a Fail
  2   | // rule on Produce answers the client with the code; switch off and on; remove; the rehearsals that happen once; Compact now.
  3   | import { choose, expect, test } from "./fixtures";
  4   | import { apiGet, apiPost, kafkaAddress, noMemory, noNativeSelect, traffic, widths } from "./r4";
  5   | 
  6   | test.describe.configure({ mode: "serial" });
  7   | 
  8   | /** Every rule in force removed and the history cleared, through the API the page uses. */
  9   | async function clean(page: import("@playwright/test").Page, baseURL: string) {
  10  |   const lab = await apiGet(page.request, "/api/v1/lab");
  11  |   for (const r of lab.home.active) await apiPost(page.request, baseURL, `/api/v1/lab/rules/${r.id}/remove`, {});
  12  |   await apiPost(page.request, baseURL, "/api/v1/lab/history/clear", {});
  13  | }
  14  | 
  15  | test("Lab: the empty state, the six rehearsals, no native control, no overflow", async ({ page, baseURL }) => {
  16  |   await clean(page, baseURL!);
  17  |   await page.goto("/lab");
  18  |   await expect(page.locator("h1")).toHaveText("Lab");
  19  |   await expect(page.getByTestId("lab-home")).toHaveAttribute("data-state", "empty");
  20  |   await expect(page.getByText("Nothing is bent")).toBeVisible();
  21  |   await expect(page.getByTestId("rehearsal-cards").locator("button")).toHaveCount(6);
  22  |   await noMemory(page);
  23  |   await noNativeSelect(page);
  24  |   await widths(page);
  25  | });
  26  | 
  27  | test("Lab: a Slow rule for one client slows its requests; the rule counts them; the sheet lists them with the lab tag", async ({ page, baseURL, browserName }) => {
  28  |   const client = `lab-slow-${browserName}`;
  29  |   const app = await traffic(await kafkaAddress(page.request), client);
  30  |   try {
  31  |     await expect.poll(() => app.sent(), { timeout: 15_000 }).toBeGreaterThan(2);
  32  |     await page.goto("/lab");
  33  |     const start = page.getByTestId("lab-new");
  34  |     await start.click();
  35  |     await expect(page).toHaveURL(/new=slow/);
  36  |     const dialog = page.getByTestId("lab-dialog");
  37  |     const form = dialog.getByTestId("lab-form-slow");
  38  |     await expect(form).toBeVisible();
  39  |     // The client, from the apps with a connection open (Combobox, mouse).
  40  |     await form.getByRole("combobox", { name: "Client" }).click();
  41  |     await page.getByRole("option", { name: client, exact: true }).click();
  42  |     await expect(form.getByRole("combobox", { name: "Client" })).toHaveText(client);
  43  |     // Topics (MultiSelect).
  44  |     await form.getByRole("combobox", { name: "Topics" }).click();
  45  |     await page.getByRole("option", { name: /^orders/ }).click();
  46  |     await page.keyboard.press("Escape");
  47  |     await expect(form.getByTestId("multi-chips").filter({ hasText: "orders" })).toBeVisible();
  48  |     await expect(form.getByTestId("multi-chips").filter({ hasText: "Produce" })).toBeVisible();
  49  |     await form.getByTestId("lab-delay").fill("300");
  50  |     await choose(page, form.getByTestId("lab-share"), "Every request");
  51  |     await form.getByTestId("lab-ends").getByRole("radio", { name: "5 min" }).click();
  52  |     await noNativeSelect(page);
  53  |     // The server's preview says what it will do.
  54  |     await expect(dialog.getByTestId("lab-preview-sentence")).toContainText(`+300 ms on every Produce request to orders from ${client}`);
  55  |     await dialog.getByTestId("lab-start").click();
  56  |     // Made: the dialog gives way to the rule's own sheet.
  57  |     const sheet = page.getByTestId("lab-rule-sheet");
  58  |     await expect(sheet).toBeVisible();
  59  |     await expect(page).toHaveURL(/rule=\d+/);
  60  |     await expect(sheet).toContainText("Slow produce to orders");
  61  |     // Its hits grow as the app writes; the requests on the sheet carry the tag and took the delay.
  62  |     await expect
  63  |       .poll(
  64  |         async () => {
  65  |           await page.getByTestId("refresh-now").click({ force: true }).catch(() => {});
  66  |           const lab = await apiGet(page.request, "/api/v1/lab");
  67  |           return lab.home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.hits ?? 0;
  68  |         },
  69  |         { timeout: 20_000 },
  70  |       )
  71  |       .toBeGreaterThan(1);
  72  |     await page.reload();
  73  |     await expect(sheet.getByRole("table", { name: "Requests the rule touched" }).locator("tbody tr").first()).toBeVisible();
  74  |     await expect(sheet.getByTestId("lab-badge").first()).toBeVisible();
  75  |     const took = await sheet.getByRole("table", { name: "Requests the rule touched" }).locator("tbody tr").first().locator("td").last().innerText();
> 76  |     expect(Number(took.replace(/[^\d]/g, ""))).toBeGreaterThanOrEqual(300);
      |                                                ^ Error: expect(received).toBeGreaterThanOrEqual(expected)
  77  |     await expect(sheet.getByTestId("tile-remaining")).toContainText(/\d:\d\d/);
  78  |     // The page's table says the same rule, its hits and its last hit as an absolute time.
  79  |     await page.keyboard.press("Escape");
  80  |     await expect(sheet).toBeHidden();
  81  |     const row = page.getByRole("table", { name: "Rules in force" }).locator("tbody tr", { hasText: "Slow produce to orders" });
  82  |     await expect(row).toBeVisible();
  83  |     await expect(row.locator("td").nth(6)).toHaveText(/\d\d:\d\d:\d\d/);
  84  | 
  85  |     // Switch it off and on with the Switch.
  86  |     const sw = row.getByTestId("rule-switch");
  87  |     await sw.click();
  88  |     await expect(sw).toHaveAttribute("aria-checked", "false");
  89  |     await expect.poll(async () => (await apiGet(page.request, "/api/v1/lab")).home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.enabled).toBe(false);
  90  |     await sw.click();
  91  |     await expect.poll(async () => (await apiGet(page.request, "/api/v1/lab")).home.active.find((r: { name: string }) => r.name === "Slow produce to orders")?.enabled).toBe(true);
  92  | 
  93  |     // Remove, with the confirm dialog; it moves to Ended.
  94  |     await row.getByTestId("rule-remove").click();
  95  |     await expect(page).toHaveURL(/remove=\d+/);
  96  |     await page.getByTestId("lab-remove-confirm").click();
  97  |     await expect(page.getByTestId("lab-remove-dialog")).toBeHidden();
  98  |     await expect(page.getByRole("table", { name: "Rules in force" }).locator("tbody tr", { hasText: "Slow produce to orders" })).toHaveCount(0);
  99  |     await expect(page.getByRole("table", { name: "Rules that ended" })).toContainText("Slow produce to orders");
  100 |     await expect(page.getByRole("table", { name: "Rules that ended" })).toContainText("Removed");
  101 |   } finally {
  102 |     await app.stop();
  103 |   }
  104 | });
  105 | 
  106 | test("Lab: a Fail rule on Produce answers the producer with the code it names", async ({ page, baseURL, browserName }) => {
  107 |   await clean(page, baseURL!);
  108 |   const client = `lab-fail-${browserName}`;
  109 |   const app = await traffic(await kafkaAddress(page.request), client, "payments.settled", 200);
  110 |   try {
  111 |     await expect.poll(() => app.sent(), { timeout: 15_000 }).toBeGreaterThan(1);
  112 |     await page.goto("/lab?new=fail");
  113 |     const form = page.getByTestId("lab-form-fail");
  114 |     await expect(form).toBeVisible();
  115 |     // APIs: Fetch out, Produce in (MultiSelect by mouse).
  116 |     await form.getByRole("combobox", { name: "APIs" }).click();
  117 |     await page.getByRole("option", { name: "Fetch" }).click();
  118 |     await page.getByRole("option", { name: "Produce" }).click();
  119 |     await page.keyboard.press("Escape");
  120 |     await expect(form.getByRole("combobox", { name: "APIs" })).toHaveText("Produce");
  121 |     await form.getByRole("combobox", { name: "Client" }).click();
  122 |     await page.getByRole("option", { name: client, exact: true }).click();
  123 |     await choose(page, form.getByTestId("lab-code"), "NOT_LEADER_OR_FOLLOWER (6)");
  124 |     await choose(page, form.getByTestId("lab-share"), "Every request");
  125 |     await expect(page.getByTestId("lab-preview-sentence")).toContainText("NOT_LEADER_OR_FOLLOWER");
  126 |     await page.getByTestId("lab-start").click();
  127 |     await expect(page.getByTestId("lab-rule-sheet")).toBeVisible();
  128 |     // The producer gets the code back (kafkajs names it by Kafka's name).
  129 |     await expect.poll(() => app.errors.filter((e) => e === "NOT_LEADER_OR_FOLLOWER").length, { timeout: 15_000 }).toBeGreaterThan(0);
  130 |     await page.reload();
  131 |     const sheet = page.getByTestId("lab-rule-sheet");
  132 |     await expect(sheet.getByRole("table", { name: "Requests the rule touched" })).toContainText("NOT_LEADER_OR_FOLLOWER");
  133 |   } finally {
  134 |     await app.stop();
  135 |     await clean(page, baseURL!);
  136 |   }
  137 | });
  138 | 
  139 | test("Lab: a field the rule cannot take is said under it, in the server's words", async ({ page }) => {
  140 |   await page.goto("/lab?new=slow");
  141 |   const form = page.getByTestId("lab-form-slow");
  142 |   await form.getByTestId("lab-delay").fill("0");
  143 |   await expect(page.getByTestId("lab-preview-none")).toContainText("Type a delay from 1 to 60,000 ms.");
  144 |   await page.getByTestId("lab-start").click();
  145 |   await expect(form.getByTestId("err-delay")).toHaveText("Type a delay from 1 to 60,000 ms.");
  146 |   await form.getByTestId("lab-hits").fill("0");
  147 |   await page.getByTestId("lab-start").click();
  148 |   await expect(form.getByTestId("err-hits")).toContainText("Type a number of hits from 1");
  149 |   // Tabs are in the address; Esc closes and Back does not reopen.
  150 |   await page.getByTestId("lab-tabs").getByRole("tab", { name: /Fail/ }).click();
  151 |   await expect(page).toHaveURL(/new=fail/);
  152 |   await page.keyboard.press("Escape");
  153 |   await expect(page.getByTestId("lab-dialog")).toBeHidden();
  154 |   await expect(page).not.toHaveURL(/new=/);
  155 | });
  156 | 
  157 | test("Lab: drop a connection, force a rebalance and expire a member, once each; each leaves a line", async ({ page, baseURL, browserName }) => {
  158 |   await clean(page, baseURL!);
  159 |   const address = await kafkaAddress(page.request);
  160 |   const client = `lab-once-${browserName}`;
  161 |   const app = await traffic(address, client, "orders", 200);
  162 |   const group = `lab-group-${browserName}`;
  163 |   const consumer = app.kafka.consumer({ groupId: group, sessionTimeout: 30_000, rebalanceTimeout: 10_000 });
  164 |   await consumer.connect();
  165 |   await consumer.subscribe({ topic: "orders", fromBeginning: false });
  166 |   await consumer.run({ eachMessage: async () => {} });
  167 |   try {
  168 |     await expect.poll(async () => (await apiGet(page.request, `/api/v1/lab/once/rebalance`)).choices?.some((c: { value: string }) => c.value === group), { timeout: 30_000 }).toBe(true);
  169 |     await page.goto("/lab");
  170 |     // Drop, once.
  171 |     await page.getByTestId("start-drop").click();
  172 |     await expect(page).toHaveURL(/new=drop/);
  173 |     const drop = page.getByTestId("lab-once-drop");
  174 |     await drop.getByRole("combobox", { name: "The app" }).click();
  175 |     await page.getByRole("option", { name: client, exact: true }).click();
  176 |     await expect(drop.getByTestId("once-call")).toContainText(`client=${client} once`);
```