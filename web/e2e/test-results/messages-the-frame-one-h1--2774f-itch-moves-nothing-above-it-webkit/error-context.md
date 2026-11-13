# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: messages.spec.ts >> the frame: one h1, the stats, the transaction region, five tabs as routes, and a tab switch moves nothing above it
- Location: tests/messages.spec.ts:8:1

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
- generic [ref=f1e2]:
  - generic [ref=f1e3]:
    - generic [ref=f1e6]:
      - link "doze-kafka, Home" [ref=f1e8]:
        - /url: /
        - generic [ref=f1e12]: doze-kafka
      - list [ref=f1e16]:
        - listitem [ref=f1e17]:
          - link "Home" [ref=f1e18]:
            - /url: /
        - listitem [ref=f1e23]:
          - link "Live" [ref=f1e24]:
            - /url: /live
        - listitem [ref=f1e28]:
          - link "Topics" [ref=f1e29]:
            - /url: /topics
          - generic: "3"
        - listitem [ref=f1e33]:
          - link "Groups" [ref=f1e34]:
            - /url: /groups
          - generic: "1"
        - listitem [ref=f1e41]:
          - link "Access" [ref=f1e42]:
            - /url: /access
        - listitem [ref=f1e46]:
          - link "Lab" [ref=f1e47]:
            - /url: /lab
        - listitem [ref=f1e51]:
          - link "Settings" [ref=f1e52]:
            - /url: /settings
      - list [ref=f1e58]:
        - listitem [ref=f1e59]:
          - link "Classic console" [ref=f1e60]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=f1e66]
    - main [ref=f1e67]:
      - generic [ref=f1e68]:
        - button "Toggle Sidebar" [ref=f1e69]
        - button "Jump to a topic or group ⌘K" [ref=f1e71]:
          - generic [ref=f1e75]: Jump to a topic or group
          - generic [ref=f1e76]: ⌘K
        - generic [ref=f1e77]:
          - button "127.0.0.1:39092" [ref=f1e78]
          - generic [ref=f1e83]: Kafka 4 profile
          - link "Open" [ref=f1e84]:
            - /url: /access
      - main [ref=f1e86]:
        - generic [ref=f1e87]:
          - navigation "Breadcrumb" [ref=f1e88]:
            - link "Topics" [ref=f1e89]:
              - /url: /topics
          - heading "No such topic" [level=1] [ref=f1e90]
        - generic [ref=f1e91]:
          - heading "There is no topic called no-such-topic-here" [level=3] [ref=f1e96]
          - generic [ref=f1e97]: Or the console may not describe it.
          - link "Back to Topics" [ref=f1e99]:
            - /url: /topics
      - generic [ref=f1e100]:
        - heading "Jump to a topic or group" [level=2] [ref=f1e101]
        - paragraph [ref=f1e102]: Type a name, then Enter
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