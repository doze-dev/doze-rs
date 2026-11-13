# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: messages.spec.ts >> the record Sheet: a row opens it with its own address, Esc and Back close it, a deep link opens it
- Location: tests/messages.spec.ts:91:1

# Error details

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
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
                - navigation:
                  - list:
                    - listitem:
                      - link:
                        - /url: /topics
                        - text: Topics
                - generic:
                  - generic: Updated 05:45:38 UTC
                  - combobox:
                    - generic: "Refresh: Off"
                  - button: Refresh
              - generic:
                - generic:
                  - heading [level=1]: orders
                  - generic: delete
                - generic:
                  - link:
                    - /url: /topics/orders/partitions
                    - text: Partitions
                  - button: Produce
                  - button: Delete topic
            - generic:
              - generic:
                - term: Partitions
                - generic:
                  - definition: "3"
                - generic: all online
              - generic:
                - term: Retention
                - generic:
                  - definition: for ever
                - generic: delete
              - generic:
                - term: Records
                - generic:
                  - definition: "17"
              - generic:
                - term: Size on disk
                - generic:
                  - definition: 1.3 KiB
              - generic:
                - term: Last write
                - generic:
                  - definition: 05:44:36
                - generic: UTC
            - generic: No open transactions.
            - navigation:
              - list:
                - listitem:
                  - link:
                    - /url: /topics/orders/messages
                    - text: Messages
                - listitem:
                  - link:
                    - /url: /topics/orders/log
                    - text: Log
                - listitem:
                  - link:
                    - /url: /topics/orders/config
                    - text: Config
                - listitem:
                  - link:
                    - /url: /topics/orders/consumers
                    - text: Consumers
                    - generic: "1"
                - listitem:
                  - link:
                    - /url: /topics/orders/producers
                    - text: Producers
                    - generic: "0"
            - generic:
              - generic:
                - generic:
                  - generic:
                    - generic:
                      - combobox:
                        - generic: All partitions
                  - search:
                    - textbox:
                      - /placeholder: Key, or offset
                  - button:
                    - generic: From time
                  - generic:
                    - generic: Reading as
                    - radiogroup:
                      - radio [checked]: read_uncommitted
                      - radio: read_committed
                - generic:
                  - generic:
                    - generic: "No filter: the newest records of every partition."
                    - generic:
                      - generic:
                        - radiogroup:
                          - radio [checked]: Snapshot
                          - radio: Live tail
                      - combobox [disabled]:
                        - generic: Relaxed
                      - generic: Snapshot
                  - generic:
                    - generic:
                      - table:
                        - rowgroup:
                          - row:
                            - columnheader:
                              - button:
                                - generic: Partition
                            - columnheader:
                              - button:
                                - generic: Offset
                            - columnheader:
                              - button:
                                - generic: Timestamp
                            - columnheader:
                              - button:
                                - generic: Key
                            - columnheader:
                              - button:
                                - generic: Value
                            - columnheader:
                              - button:
                                - generic: Value size
                            - columnheader:
                              - button:
                                - generic: For read_committed
                        - rowgroup:
                          - row:
                            - cell: "1"
                            - cell:
                              - button: "4"
                            - cell: 05:44:36.691
                            - cell: tail-b-1794548676691-0
                            - cell: "{\"tag\":\"tail-b\",\"i\":0,\"at\":1794548676691}"
                            - cell: 41 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "6"
                            - cell: 05:44:36.691
                            - cell: tail-b-1794548676691-1
                            - cell: "{\"tag\":\"tail-b\",\"i\":1,\"at\":1794548676691}"
                            - cell: 41 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "5"
                            - cell: 05:44:35.831
                            - cell: tail-a-1794548675821-1
                            - cell: "{\"tag\":\"tail-a\",\"i\":1,\"at\":1794548675821}"
                            - cell: 41 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "4"
                            - cell: 05:44:35.831
                            - cell: tail-a-1794548675821-0
                            - cell: "{\"tag\":\"tail-a\",\"i\":0,\"at\":1794548675821}"
                            - cell: 41 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "1"
                            - cell:
                              - button: "3"
                            - cell: 05:44:35.830
                            - cell: tail-a-1794548675821-2
                            - cell: "{\"tag\":\"tail-a\",\"i\":2,\"at\":1794548675821}"
                            - cell: 41 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "0"
                            - cell:
                              - button: "4"
                            - cell: 05:44:11.106
                            - cell: orders-9
                            - cell: "{\"i\":9,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "0"
                            - cell:
                              - button: "3"
                            - cell: 05:44:11.106
                            - cell: orders-5
                            - cell: "{\"i\":5,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "0"
                            - cell:
                              - button: "2"
                            - cell: 05:44:11.106
                            - cell: orders-2
                            - cell: "{\"i\":2,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "0"
                            - cell:
                              - button: "1"
                            - cell: 05:44:11.106
                            - cell: orders-1
                            - cell: "{\"i\":1,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "0"
                            - cell:
                              - button: "0"
                            - cell: 05:44:11.106
                            - cell: orders-0
                            - cell: "{\"i\":0,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "1"
                            - cell:
                              - button: "2"
                            - cell: 05:44:11.106
                            - cell: orders-7
                            - cell: "{\"i\":7,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "1"
                            - cell:
                              - button: "1"
                            - cell: 05:44:11.106
                            - cell: orders-6
                            - cell: "{\"i\":6,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "1"
                            - cell:
                              - button: "0"
                            - cell: 05:44:11.106
                            - cell: orders-4
                            - cell: "{\"i\":4,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "3"
                            - cell: 05:44:11.106
                            - cell: orders-11
                            - cell: "{\"i\":11,\"topic\":\"orders\"}"
                            - cell: 25 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "2"
                            - cell: 05:44:11.106
                            - cell: orders-10
                            - cell: "{\"i\":10,\"topic\":\"orders\"}"
                            - cell: 25 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "1"
                            - cell: 05:44:11.106
                            - cell: orders-8
                            - cell: "{\"i\":8,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                          - row:
                            - cell: "2"
                            - cell:
                              - button: "0"
                            - cell: 05:44:11.106
                            - cell: orders-3
                            - cell: "{\"i\":3,\"topic\":\"orders\"}"
                            - cell: 24 B
                            - cell:
                              - generic: Committed
                    - generic: Newest first · 17 of 17
                  - paragraph:
                    - generic: 3 of 3 partitions online
                  - generic:
                    - button: How this read works
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "orders-0 · 999,999,999" [active] [ref=f2e2]:
    - generic [ref=f2e3]:
      - heading "orders-0 · 999,999,999" [level=2] [ref=f2e4]
      - paragraph [ref=f2e5]: One record, completely.
    - generic [ref=f2e6]:
      - paragraph [ref=f2e7]: There is no record at offset 999,999,999 of orders-0. The offset may be past the end of the log or before its start, or the record may have been removed by compaction, by a deletion, or be a transaction marker.
      - button "Back to the messages" [ref=f2e8]
    - button "Close" [ref=f2e9]
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