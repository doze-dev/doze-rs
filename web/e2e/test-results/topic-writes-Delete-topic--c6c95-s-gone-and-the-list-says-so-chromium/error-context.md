# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Delete topic >> type the name to confirm; then the topic is gone and the list says so
- Location: tests/topic-writes.spec.ts:236:3

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
          - generic: "51"
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
          - generic [ref=e88]:
            - navigation "Breadcrumb"
          - heading "Topics" [level=1] [ref=e91]
        - generic [ref=e92]:
          - heading "Being rebuilt" [level=3] [ref=e98]
          - generic [ref=e99]: This page moves to the new console in slice R1. Until then the classic console has it, with the same data.
          - generic [ref=e100]:
            - link "Open it in the classic console" [ref=e101] [cursor=pointer]:
              - /url: /classic/topics
            - link "Home" [ref=e102] [cursor=pointer]:
              - /url: /
      - generic [ref=e103]:
        - heading "Jump to a topic or group" [level=2] [ref=e104]
        - paragraph [ref=e105]: Type a name, then Enter
  - region "Notifications alt+T":
    - list:
      - listitem [ref=e106]:
        - generic [ref=e111]:
          - generic [ref=e112]: Deleted r1b-gone-chromium-mwe8n90o-1.
          - generic [ref=e113]: "1 record and 2 partitions are gone from disk. A topic named r1b-gone-chromium-mwe8n90o-1 made later is a new topic: it starts at offset 0."
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