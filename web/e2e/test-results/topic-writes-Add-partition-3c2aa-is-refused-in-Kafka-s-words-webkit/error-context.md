# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Add partitions >> the new total, what it adds, then the broker's count; fewer is refused in Kafka's words
- Location: tests/topic-writes.spec.ts:179:3

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
                      - generic: "33"
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
                - heading [level=1]: r1b-grow-webkit-mwe8lozp-1
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-grow-webkit-mwe8lozp-1
              - text: Messages
            - link:
              - /url: /topics/r1b-grow-webkit-mwe8lozp-1/log
              - text: Log
            - link:
              - /url: /topics/r1b-grow-webkit-mwe8lozp-1/config
              - text: Config
            - link:
              - /url: /topics/r1b-grow-webkit-mwe8lozp-1/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-grow-webkit-mwe8lozp-1/producers
              - text: Producers
          - paragraph:
            - text: Messages is being rebuilt (slice R1a).
            - link:
              - /url: /classic/topics/r1b-grow-webkit-mwe8lozp-1
              - text: Open it in the classic console
            - text: .
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Add partitions to r1b-grow-webkit-mwe8lozp-1" [active] [ref=f1e2]:
    - generic [ref=f1e3]:
      - heading [level=2] [ref=f1e4]:
        - text: Add partitions to
        - code [ref=f1e5]: r1b-grow-webkit-mwe8lozp-1
      - paragraph [ref=f1e6]: The new total. Partitions can be added, never removed.
    - generic [ref=f1e8]:
      - generic [ref=f1e9]:
        - generic [ref=f1e10]:
          - text: Partitions after
          - generic [ref=f1e11]: now 3
        - textbox "Partitions after now 3" [invalid] [ref=f1e13]: "2"
        - paragraph [ref=f1e14]: 2 is below the 3 partitions there are.
      - alert [ref=f1e15]:
        - generic [ref=f1e20]:
          - generic [ref=f1e21]:
            - generic [ref=f1e22]: Refused
            - code [ref=f1e23]: INVALID_PARTITIONS (37)
          - paragraph [ref=f1e24]: The topic r1b-grow-webkit-mwe8lozp-1 currently has 3 partition(s); 2 would not be an increase.
          - paragraph [ref=f1e25]: A topic's partition count can only go up. The same count is refused too. Nothing changed.
          - paragraph [ref=f1e26]: "What would pass: any total from 4 to 1,000."
      - region "The answer" [ref=f1e27]:
        - heading "The answer" [level=3] [ref=f1e28]
        - generic [ref=f1e34]:
          - generic [ref=f1e35]: CreatePartitions v3 response
          - generic [ref=f1e36]: "name: r1b-grow-webkit-mwe8lozp-1"
          - generic [ref=f1e37]: "error_code: 37 INVALID_PARTITIONS"
          - generic [ref=f1e38]: "error_message: The topic r1b-grow-webkit-mwe8lozp-1 currently has 3 partition(s); 2 would not be an increase."
      - generic [ref=f1e39]:
        - button "Cancel" [ref=f1e40]
        - button "Add partitions" [ref=f1e41]
    - button "Close" [ref=f1e42]
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