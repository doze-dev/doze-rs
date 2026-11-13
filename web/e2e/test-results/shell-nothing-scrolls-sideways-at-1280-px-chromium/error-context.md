# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: shell.spec.ts >> nothing scrolls sideways at 1280 px
- Location: tests/shell.spec.ts:126:3

# Error details

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 4

- Array []
+ Array [
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+ ]
```

# Page snapshot

```yaml
- generic [ref=f7e2]:
  - generic [ref=f7e3]:
    - generic [ref=f7e6]:
      - link "doze-kafka, Home" [ref=f7e8] [cursor=pointer]:
        - /url: /
        - generic [ref=f7e12]: doze-kafka
      - list [ref=f7e16]:
        - listitem [ref=f7e17]:
          - link "Home" [ref=f7e18] [cursor=pointer]:
            - /url: /
        - listitem [ref=f7e23]:
          - link "Live" [ref=f7e24] [cursor=pointer]:
            - /url: /live
        - listitem [ref=f7e28]:
          - link "Topics" [ref=f7e29] [cursor=pointer]:
            - /url: /topics
          - generic: "3"
        - listitem [ref=f7e33]:
          - link "Groups" [ref=f7e34] [cursor=pointer]:
            - /url: /groups
          - generic: "1"
        - listitem [ref=f7e41]:
          - link "Access" [ref=f7e42] [cursor=pointer]:
            - /url: /access
        - listitem [ref=f7e46]:
          - link "Lab" [ref=f7e47] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=f7e51]:
          - link "Settings" [ref=f7e52] [cursor=pointer]:
            - /url: /settings
      - list [ref=f7e58]:
        - listitem [ref=f7e59]:
          - link "Classic console" [ref=f7e60] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=f7e66]
    - main [ref=f7e67]:
      - generic [ref=f7e68]:
        - button "Toggle Sidebar" [ref=f7e69]
        - button "Jump to a topic or group ⌘K" [ref=f7e71]:
          - generic [ref=f7e75]: Jump to a topic or group
          - generic [ref=f7e76]: ⌘K
        - generic [ref=f7e77]:
          - button "127.0.0.1:39092" [ref=f7e78]
          - generic [ref=f7e83]: Kafka 4 profile
          - link "Open" [ref=f7e84] [cursor=pointer]:
            - /url: /access
      - main [ref=f7e86]:
        - generic [ref=f7e87]:
          - navigation "Breadcrumb" [ref=f7e88]:
            - link "Topics" [ref=f7e89] [cursor=pointer]:
              - /url: /topics
          - heading "No such topic" [level=1] [ref=f7e90]
        - generic [ref=f7e91]:
          - heading "There is no topic called a-very-long-topic-name-that-needs-an-ellipsis.in-every-narrow-place.v2" [level=3] [ref=f7e96]
          - generic [ref=f7e97]: Or the console may not describe it.
          - link "Back to Topics" [ref=f7e99] [cursor=pointer]:
            - /url: /topics
      - generic [ref=f7e100]:
        - heading "Jump to a topic or group" [level=2] [ref=f7e101]
        - paragraph [ref=f7e102]: Type a name, then Enter
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