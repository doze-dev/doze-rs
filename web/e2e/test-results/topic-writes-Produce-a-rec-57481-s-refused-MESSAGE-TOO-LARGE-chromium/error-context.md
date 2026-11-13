# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Produce >> a record over max.message.bytes: refused MESSAGE_TOO_LARGE
- Location: tests/topic-writes.spec.ts:164:3

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
                      - generic: "48"
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
                - heading [level=1]: r1b-large-chromium-mwe8n4g3-1
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-large-chromium-mwe8n4g3-1
              - text: Messages
            - link:
              - /url: /topics/r1b-large-chromium-mwe8n4g3-1/log
              - text: Log
            - link:
              - /url: /topics/r1b-large-chromium-mwe8n4g3-1/config
              - text: Config
            - link:
              - /url: /topics/r1b-large-chromium-mwe8n4g3-1/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-large-chromium-mwe8n4g3-1/producers
              - text: Producers
          - paragraph:
            - text: Messages is being rebuilt (slice R1a).
            - link:
              - /url: /classic/topics/r1b-large-chromium-mwe8n4g3-1
              - text: Open it in the classic console
            - text: .
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Produce to r1b-large-chromium-mwe8n4g3-1" [active] [ref=e2]:
    - generic [ref=e3]:
      - heading [level=2] [ref=e4]:
        - text: Produce to
        - code [ref=e5]: r1b-large-chromium-mwe8n4g3-1
      - paragraph [ref=e6]: One record. The call below is the one that will be sent; the broker chooses the offset.
    - generic [ref=e8]:
      - generic [ref=e9]:
        - generic [ref=e10]:
          - generic [ref=e11]: Partition
          - combobox "Partition" [ref=e12]:
            - generic: auto (the partitioner picks)
          - combobox [aria-hidden] [ref=e13]
          - paragraph [ref=e14]: Auto, or 0 to 0. Auto sends a key where Kafka's default partitioner does.
        - generic [ref=e15]:
          - generic [ref=e16]:
            - text: Key
            - generic [ref=e17]: optional on this topic
          - textbox "Key optional on this topic" [ref=e18]: k
      - generic [ref=e19]:
        - generic [ref=e20]: Value
        - textbox "Value" [ref=e21]:
          - /placeholder: "{\"order_id\": \"ord-1\"}"
          - text: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
        - paragraph [ref=e22]: Text or JSON. Leave it empty for a record with no value (on a compacted topic, a deletion).
      - group "Headers up to 8" [ref=e23]:
        - generic [ref=e25]:
          - textbox "Header name 1" [ref=e26]:
            - /placeholder: name
          - textbox "Header value 1" [ref=e27]:
            - /placeholder: value
          - button "Remove header 1" [ref=e28]
        - button "Add header" [ref=e30]
      - generic [ref=e31]:
        - generic [ref=e32]: acks
        - radiogroup "acks" [ref=e33]:
          - radio "all (-1)" [checked] [ref=e34]
          - radio "1" [ref=e35]
          - radio "0" [ref=e36]
      - button "What Kafka says about these fields" [ref=e38]
      - generic [ref=e41]:
        - paragraph [ref=e42]: The record was not written. No offset was used.
        - alert [ref=e43]:
          - generic [ref=e48]:
            - generic [ref=e49]:
              - generic [ref=e50]: Refused
              - code [ref=e51]: MESSAGE_TOO_LARGE (10)
            - paragraph [ref=e52]: The request included a message larger than the max message size the server will accept.
            - paragraph [ref=e53]: The record is larger than this topic accepts. The batch was not written; no offset was used.
            - paragraph [ref=e54]: "What would pass: a record under 200 bytes (max.message.bytes), or a larger limit on the topic."
        - region "The answer" [ref=e55]:
          - heading "The answer" [level=3] [ref=e56]
          - generic [ref=e62]:
            - generic [ref=e63]: Produce v9 response
            - generic [ref=e64]: "topic: r1b-large-chromium-mwe8n4g3-1"
            - generic [ref=e65]: "partition: 0"
            - generic [ref=e66]: "error_code: 10 MESSAGE_TOO_LARGE"
            - generic [ref=e67]: "base_offset: -1"
      - generic [ref=e68]:
        - button "Cancel" [ref=e69]
        - button "Send again" [ref=e70]
    - button "Close" [ref=e71]
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