# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Delete records >> on a topic that only compacts: refused POLICY_VIOLATION
- Location: tests/topic-writes.spec.ts:221:3

# Error details

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "console: Failed to load resource: the server responded with a status of 422 (Unprocessable Entity)",
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
                      - generic: "51"
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
              - navigation:
                - list:
                  - listitem:
                    - link:
                      - /url: /topics
                      - text: Topics
            - generic:
              - generic:
                - heading [level=1]: r1b-trimc-chromium-mwe8n7zd-2
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-trimc-chromium-mwe8n7zd-2
              - text: Messages
            - link:
              - /url: /topics/r1b-trimc-chromium-mwe8n7zd-2/log
              - text: Log
            - link:
              - /url: /topics/r1b-trimc-chromium-mwe8n7zd-2/config
              - text: Config
            - link:
              - /url: /topics/r1b-trimc-chromium-mwe8n7zd-2/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-trimc-chromium-mwe8n7zd-2/producers
              - text: Producers
          - paragraph:
            - text: Messages is being rebuilt (slice R1a).
            - link:
              - /url: /classic/topics/r1b-trimc-chromium-mwe8n7zd-2
              - text: Open it in the classic console
            - text: .
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Delete records from r1b-trimc-chromium-mwe8n7zd-2" [ref=e2]:
    - generic [ref=e3]:
      - heading [level=2] [ref=e4]:
        - text: Delete records from
        - code [ref=e5]: r1b-trimc-chromium-mwe8n7zd-2
      - paragraph [ref=e6]: Moves a partition's log start forward. Earlier records are no longer served.
    - generic [ref=e8]:
      - generic [ref=e9]:
        - generic [ref=e10]:
          - generic [ref=e11]: Partition
          - combobox "Partition" [ref=e12]
          - combobox [aria-hidden] [ref=e13]
        - generic [ref=e14]:
          - generic [ref=e15]: Delete everything before offset
          - generic [ref=e16]:
            - textbox "Delete everything before offset" [ref=e17]:
              - /placeholder: "0"
              - text: "1"
            - generic: start 0 · end 2
      - paragraph [ref=e21]: This removes 0 records. The log of r1b-trimc-chromium-mwe8n7zd-2-0 already starts at 0 or later, so nothing changes.
      - button "What Kafka says about these fields" [ref=e23]
      - generic [ref=e26]:
        - paragraph [ref=e27]: "Delete records on a topic that only compacts. The partition was not touched: log start is still 0."
        - alert [ref=e28]:
          - generic [ref=e33]:
            - generic [ref=e34]:
              - generic [ref=e35]: Refused
              - code [ref=e36]: POLICY_VIOLATION (44)
            - paragraph [ref=e37]: Request parameters do not satisfy the configured policy.
            - paragraph [ref=e38]: On a topic whose cleanup.policy is only compact, records are removed by the cleaner (the latest value of each key stays), not by offset. This broker answers this for every partition of such a topic.
            - paragraph [ref=e39]:
              - text: "What would pass: add"
              - code [ref=e40]: delete
              - text: to cleanup.policy (
              - code [ref=e41]: delete,compact
              - text: ), then records before an offset can go.
        - region "The answer" [ref=e42]:
          - heading "The answer" [level=3] [ref=e43]
          - generic [ref=e49]:
            - generic [ref=e50]: DeleteRecords v2 response
            - generic [ref=e51]: "topic: r1b-trimc-chromium-mwe8n7zd-2"
            - generic [ref=e52]: "partition_index: 0"
            - generic [ref=e53]: "low_watermark: -1"
            - generic [ref=e54]: "error_code: 44 POLICY_VIOLATION"
      - generic [ref=e55]:
        - button "Cancel" [ref=e56]
        - button "Delete records" [active] [ref=e57]
    - button "Close" [ref=e58]
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