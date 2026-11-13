# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Produce >> a keyless record to a compacted topic: refused INVALID_RECORD in Kafka's words, the key field says why
- Location: tests/topic-writes.spec.ts:146:3

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
                      - generic: "47"
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
                - heading [level=1]: r1b-compacted-chromium-mwe8n3bt-3
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-compacted-chromium-mwe8n3bt-3
              - text: Messages
            - link:
              - /url: /topics/r1b-compacted-chromium-mwe8n3bt-3/log
              - text: Log
            - link:
              - /url: /topics/r1b-compacted-chromium-mwe8n3bt-3/config
              - text: Config
            - link:
              - /url: /topics/r1b-compacted-chromium-mwe8n3bt-3/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-compacted-chromium-mwe8n3bt-3/producers
              - text: Producers
          - paragraph:
            - text: Messages is being rebuilt (slice R1a).
            - link:
              - /url: /classic/topics/r1b-compacted-chromium-mwe8n3bt-3
              - text: Open it in the classic console
            - text: .
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Produce to r1b-compacted-chromium-mwe8n3bt-3" [active] [ref=e2]:
    - generic [ref=e3]:
      - heading [level=2] [ref=e4]:
        - text: Produce to
        - code [ref=e5]: r1b-compacted-chromium-mwe8n3bt-3
      - paragraph [ref=e6]: One record. The call below is the one that will be sent; the broker chooses the offset.
    - generic [ref=e8]:
      - status [ref=e9]:
        - generic [ref=e13]:
          - generic [ref=e14]:
            - text: Sent to
            - code [ref=e15]: r1b-compacted-chromium-mwe8n3bt-3-0
            - text: at offset 0.
          - text: Timestamp 00:28:06.672 UTC, acknowledged after the flush (acks -1).
      - generic [ref=e16]:
        - term [ref=e17]: Partition
        - definition [ref=e18]: "0"
        - term [ref=e19]: Offset
        - definition [ref=e20]: "0"
        - term [ref=e21]: Key
        - definition [ref=e22]: k
        - term [ref=e23]: Value
        - definition [ref=e24]: v
        - term [ref=e25]: For read_committed
        - definition [ref=e26]: Committed
      - generic [ref=e27]:
        - heading "Is it visible?" [level=3] [ref=e28]
        - paragraph [ref=e29]: Yes, to both isolation levels.
        - paragraph [ref=e30]: "r1b-compacted-chromium-mwe8n3bt-3-0 has no open transaction below this record: its last stable offset is 1 and its high watermark 1. A consumer reading read_committed gets offset 0 as soon as it asks."
      - region "The answer" [ref=e31]:
        - heading "The answer" [level=3] [ref=e32]
        - generic [ref=e38]:
          - generic [ref=e39]: Produce v9 response
          - generic [ref=e40]: "topic: r1b-compacted-chromium-mwe8n3bt-3"
          - generic [ref=e41]: "partition: 0"
          - generic [ref=e42]: "error_code: 0"
          - generic [ref=e43]: "base_offset: 0"
          - generic [ref=e44]: "log_append_time_ms: -1"
          - generic [ref=e45]: "log_start_offset: 0"
        - paragraph [ref=e46]: The log chose the offset; the producer only supplies the record. -1 means the topic keeps the producer's time (CreateTime).
      - generic [ref=e47]:
        - button "Produce another" [ref=e48]
        - button "Done" [ref=e49]
    - button "Close" [ref=e50]
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