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

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "pageerror: ResizeObserver loop completed with undelivered notifications.",
+ ]
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
                - generic: Updated 05:29:21 UTC
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
                  - definition: "2"
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
                      - cell: "2"
                      - cell: 05:29:20
                      - cell: 05:34
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
      - paragraph [ref=f1e7]: +300 ms on every Produce request to orders from lab-slow-webkit.
    - generic [ref=f1e8]:
      - generic [ref=f1e9]:
        - generic [ref=f1e10]:
          - switch "Rule 2 on" [checked] [ref=f1e11]
          - text: "On: it fires on the requests it matches"
        - button "Remove rule" [ref=f1e12]
      - generic [ref=f1e13]:
        - generic [ref=f1e14]:
          - term [ref=f1e15]: Hits
          - definition [ref=f1e17]: "2"
          - generic "of 2 matched · every request" [ref=f1e18]
        - generic [ref=f1e19]:
          - term [ref=f1e20]: Last hit
          - definition [ref=f1e22]: 05:29:20
          - generic "UTC" [ref=f1e23]
        - generic [ref=f1e24]:
          - term [ref=f1e25]: Time left
          - definition [ref=f1e26]:
            - time [ref=f1e27]: 4:59
          - generic [ref=f1e28]: ends 05:34 UTC
        - generic [ref=f1e29]:
          - term [ref=f1e30]: Added delay
          - definition [ref=f1e32]: "300"
          - generic "ms each" [ref=f1e33]
      - generic [ref=f1e34]:
        - heading "The last hits" [level=3] [ref=f1e35]
        - paragraph [ref=f1e36]: The same rows as on Live, only the ones this rule touched.
        - table "Requests the rule touched" [ref=f1e38]:
          - rowgroup [ref=f1e39]:
            - row [ref=f1e40]:
              - columnheader "Time" [ref=f1e41]:
                - button "Time" [ref=f1e42]
              - columnheader "Client" [ref=f1e45]:
                - button "Client" [ref=f1e46]
              - columnheader "Request" [ref=f1e49]:
                - button "Request" [ref=f1e50]
              - columnheader "Target" [ref=f1e53]:
                - button "Target" [ref=f1e54]
              - columnheader "Result" [ref=f1e57]:
                - button "Result" [ref=f1e58]
              - columnheader "Took" [ref=f1e61]:
                - button "Took" [ref=f1e62]
              - columnheader "Lab" [ref=f1e65]
          - rowgroup [ref=f1e67]:
            - row [ref=f1e68]:
              - cell "05:29:20" [ref=f1e69]
              - cell "lab-slow-webkit" [ref=f1e70]
              - cell "Produce v7" [ref=f1e71]
              - cell "orders-2" [ref=f1e72]
              - cell [ref=f1e73]:
                - generic "1 record" [ref=f1e74]: Done
              - cell "303 ms" [ref=f1e77]
              - cell [ref=f1e78]:
                - 'link "Lab: Slow produce to orders, made the answer wait 300 ms more" [ref=f1e79]':
                  - /url: /lab?rule=2
                  - text: lab
            - row [ref=f1e82]:
              - cell "05:29:20" [ref=f1e83]
              - cell "lab-slow-webkit" [ref=f1e84]
              - cell "Produce v7" [ref=f1e85]
              - cell "orders-1" [ref=f1e86]
              - cell [ref=f1e87]:
                - generic "1 record" [ref=f1e88]: Done
              - cell "302 ms" [ref=f1e91]
              - cell [ref=f1e92]:
                - 'link "Lab: Slow produce to orders, made the answer wait 300 ms more" [ref=f1e93]':
                  - /url: /lab?rule=2
                  - text: lab
      - paragraph [ref=f1e96]: "The first hit: lab-slow-webkit, Produce v7 to orders-1 at 05:29:20 UTC."
      - paragraph [ref=f1e97]: "Apps it touched: lab-slow-webkit."
      - paragraph [ref=f1e98]: Nothing is wrong with the broker. The rule only changes the answer, so no record changes and no offset moves. When it ends, requests behave as before with no restart.
    - button "Close" [ref=f1e99]
```

# Test source

```ts
  1  | import { test as base, expect, type Page } from "@playwright/test";
  2  | 
  3  | /** Every test fails on a console error or an uncaught exception in the page (the owner's bar: no console errors). */
  4  | export const test = base.extend<{ errors: string[] }>({
  5  |   errors: [
  6  |     async ({ page }, use) => {
  7  |       const errors: string[] = [];
  8  |       page.on("console", (m) => {
  9  |         // A font download that the next navigation cut short (Firefox: NS_BINDING_ABORTED, 0x804b0002) is the test moving
  10 |         // faster than a person, not a fault of the page.
  11 |         if (m.type() === "error" && !/downloadable font: download failed.*status=2152398850/.test(m.text())) errors.push(`console: ${m.text()}`);
  12 |       });
  13 |       page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  14 |       await use(errors);
> 15 |       expect(errors, "no console errors").toEqual([]);
     |                                           ^ Error: no console errors
  16 |     },
  17 |     { auto: true },
  18 |   ],
  19 | });
  20 | 
  21 | export { expect };
  22 | 
  23 | /** The page scrolls sideways by this many pixels (0 is the rule). */
  24 | export async function overflowX(page: Page): Promise<number> {
  25 |   return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  26 | }
  27 | 
  28 | /** Opens a shadcn Select by its trigger and chooses an option by its visible name, with the mouse. */
  29 | export async function choose(page: Page, trigger: ReturnType<Page["locator"]>, option: string) {
  30 |   await trigger.click();
  31 |   await page.getByRole("option", { name: option, exact: true }).click();
  32 | }
  33 | 
  34 | /** The rectangles of the given locators, rounded. */
  35 | export async function boxes(page: Page, selectors: string[]) {
  36 |   return page.evaluate(
  37 |     (sels) =>
  38 |       sels.map((s) => {
  39 |         const r = document.querySelector(s)?.getBoundingClientRect();
  40 |         return r ? [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)] : null;
  41 |       }),
  42 |     selectors,
  43 |   );
  44 | }
  45 | 
```