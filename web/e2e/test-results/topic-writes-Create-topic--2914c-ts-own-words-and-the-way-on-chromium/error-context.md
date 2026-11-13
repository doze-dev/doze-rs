# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Create topic >> a topic made while the form is open: Kafka refuses with its own words, and the way on
- Location: tests/topic-writes.spec.ts:70:3

# Error details

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 4

- Array []
+ Array [
+   "console: Failed to load resource: the server responded with a status of 422 (Unprocessable Entity)",
+   "console: Failed to load resource: the server responded with a status of 422 (Unprocessable Entity)",
+ ]
```

# Page snapshot

```yaml
- generic [ref=e2]:
  - generic [ref=e3]:
    - generic [ref=e6]:
      - link "doze-kafka, Home" [ref=e8] [cursor=pointer]:
        - /url: /
        - generic [ref=e12]: doze-kafka
      - list [ref=e16]:
        - listitem [ref=e17]:
          - link "Home" [ref=e18] [cursor=pointer]:
            - /url: /
        - listitem [ref=e23]:
          - link "Live" [ref=e24] [cursor=pointer]:
            - /url: /live
        - listitem [ref=e28]:
          - link "Topics" [ref=e29] [cursor=pointer]:
            - /url: /topics
          - generic: "43"
        - listitem [ref=e33]:
          - link "Groups" [ref=e34] [cursor=pointer]:
            - /url: /groups
          - generic: "1"
        - listitem [ref=e41]:
          - link "Access" [ref=e42] [cursor=pointer]:
            - /url: /access
        - listitem [ref=e46]:
          - link "Lab" [ref=e47] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=e51]:
          - link "Settings" [ref=e52] [cursor=pointer]:
            - /url: /settings
      - list [ref=e58]:
        - listitem [ref=e59]:
          - link "Classic console" [ref=e60] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=e66]
    - main [ref=e67]:
      - generic [ref=e68]:
        - button "Toggle Sidebar" [ref=e69]
        - button "Jump to a topic or group ⌘K" [ref=e71]:
          - generic [ref=e75]: Jump to a topic or group
          - generic [ref=e76]: ⌘K
        - generic [ref=e77]:
          - button "127.0.0.1:39092" [ref=e78]
          - generic [ref=e83]: Kafka 4 profile
          - link "Open" [ref=e84] [cursor=pointer]:
            - /url: /access
      - main [ref=e86]:
        - generic [ref=e87]:
          - navigation "Breadcrumb" [ref=e89]:
            - list [ref=e90]:
              - listitem [ref=e91]:
                - link "Topics" [ref=e92] [cursor=pointer]:
                  - /url: /topics
          - generic [ref=e93]:
            - heading "r1b-race-chromium-mwe8mzjx-1" [level=1] [ref=e95]
            - generic [ref=e96]:
              - button "Add partitions" [ref=e97]
              - button "Delete records" [ref=e98]
              - button "Produce" [ref=e99]
              - button "Delete topic" [ref=e100]
        - navigation "Topic" [ref=e101]:
          - link "Messages" [ref=e102] [cursor=pointer]:
            - /url: /topics/r1b-race-chromium-mwe8mzjx-1
          - link "Log" [ref=e103] [cursor=pointer]:
            - /url: /topics/r1b-race-chromium-mwe8mzjx-1/log
          - link "Config" [ref=e104] [cursor=pointer]:
            - /url: /topics/r1b-race-chromium-mwe8mzjx-1/config
          - link "Consumers" [ref=e105] [cursor=pointer]:
            - /url: /topics/r1b-race-chromium-mwe8mzjx-1/consumers
          - link "Producers" [ref=e106] [cursor=pointer]:
            - /url: /topics/r1b-race-chromium-mwe8mzjx-1/producers
        - paragraph [ref=e107]:
          - text: Messages is being rebuilt (slice R1a).
          - link "Open it in the classic console" [ref=e108] [cursor=pointer]:
            - /url: /classic/topics/r1b-race-chromium-mwe8mzjx-1
          - text: .
      - generic [ref=e109]:
        - heading "Jump to a topic or group" [level=2] [ref=e110]
        - paragraph [ref=e111]: Type a name, then Enter
  - region "Notifications alt+T"
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