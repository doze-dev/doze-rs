# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-tabs.spec.ts >> Producers: the stats, the partition in the address, a producer's batches open in place; a transaction that is not there says so
- Location: tests/topic-tabs.spec.ts:85:1

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
      - main [active] [ref=f1e86]:
        - generic [ref=f1e87]:
          - generic [ref=f1e88]:
            - generic [ref=f1e89]:
              - navigation "Breadcrumb" [ref=f1e90]:
                - list [ref=f1e91]:
                  - listitem [ref=f1e92]:
                    - link "Topics" [ref=f1e93]:
                      - /url: /topics
              - generic [ref=f1e94]:
                - generic [ref=f1e95]: Updated 05:46:23 UTC
                - combobox "Refresh every" [ref=f1e96]:
                  - generic: "Refresh: Off"
                - button "Refresh" [ref=f1e97]
            - generic [ref=f1e98]:
              - generic [ref=f1e99]:
                - heading "orders" [level=1] [ref=f1e100]
                - generic [ref=f1e101]: delete
              - generic [ref=f1e103]:
                - link "Partitions" [ref=f1e104]:
                  - /url: /topics/orders/partitions
                - button "Produce" [ref=f1e105]
                - button "Delete topic" [ref=f1e106]
          - generic [ref=f1e107]:
            - generic [ref=f1e108]:
              - term [ref=f1e109]: Partitions
              - definition [ref=f1e111]: "3"
              - generic "all online" [ref=f1e112]
            - generic [ref=f1e113]:
              - term [ref=f1e114]: Retention
              - definition [ref=f1e116]: for ever
              - generic "delete" [ref=f1e117]
            - generic [ref=f1e118]:
              - term [ref=f1e119]: Records
              - definition [ref=f1e121]: "22"
            - generic [ref=f1e122]:
              - term [ref=f1e123]: Size on disk
              - definition [ref=f1e125]: 2.1 KiB
            - generic [ref=f1e126]:
              - term [ref=f1e127]: Last write
              - definition [ref=f1e129]: 05:45:41
              - generic "UTC" [ref=f1e130]
          - generic [ref=f1e131]: No open transactions.
          - navigation "Topic" [ref=f1e136]:
            - list [ref=f1e137]:
              - listitem [ref=f1e138]:
                - link "Messages" [ref=f1e139]:
                  - /url: /topics/orders/messages
              - listitem [ref=f1e140]:
                - link "Log" [ref=f1e141]:
                  - /url: /topics/orders/log
              - listitem [ref=f1e142]:
                - link "Config" [ref=f1e143]:
                  - /url: /topics/orders/config
              - listitem [ref=f1e144]:
                - link "Consumers 1" [ref=f1e145]:
                  - /url: /topics/orders/consumers
                  - text: Consumers
                  - generic [ref=f1e146]: "1"
              - listitem [ref=f1e147]:
                - link "Producers 0" [ref=f1e148]:
                  - /url: /topics/orders/producers
                  - text: Producers
                  - generic [ref=f1e149]: "0"
          - generic [ref=f1e151]:
            - generic [ref=f1e152]:
              - generic [ref=f1e153]: partition 0 of 3
              - radiogroup "Partition" [ref=f1e154]:
                - radio "orders-0" [checked] [ref=f1e155]
                - radio "orders-1" [ref=f1e156]
                - radio "orders-2" [ref=f1e157]
            - generic [ref=f1e158]:
              - generic [ref=f1e159]:
                - term [ref=f1e160]: Producers
                - definition [ref=f1e162]: "0"
              - generic [ref=f1e163]:
                - term [ref=f1e164]: Open transactions
                - definition [ref=f1e166]: "0"
              - generic [ref=f1e167]:
                - term [ref=f1e168]: Last stable
                - definition [ref=f1e170]: "7"
              - generic [ref=f1e171]:
                - term [ref=f1e172]: High watermark
                - definition [ref=f1e174]: "7"
              - generic [ref=f1e175]:
                - term [ref=f1e176]: Held back
                - definition [ref=f1e178]: "0"
                - generic "records" [ref=f1e179]
            - generic [ref=f1e180]:
              - heading "Producers on orders-0" [level=2] [ref=f1e181]
              - paragraph [ref=f1e182]: "What DescribeProducers says: the id, its epoch, the last sequence the partition accepted, and when it last wrote. Open a row to see the last batches the partition remembers for that producer."
              - table "Producers" [ref=f1e183]:
                - generic [ref=f1e184]:
                  - row [ref=f1e185]:
                    - columnheader "Producer id" [ref=f1e186]
                    - columnheader "Epoch" [ref=f1e187]
                    - columnheader "Last sequence" [ref=f1e188]
                    - columnheader "Idle" [ref=f1e189]
                    - columnheader "Transaction" [ref=f1e190]
                    - columnheader "Seen as" [ref=f1e191]
                  - paragraph [ref=f1e192]: No producer has written to this partition lately.
              - paragraph [ref=f1e193]: The broker keeps producer ids, not client ids. A producer id silent for 1 day is forgotten on the partition.
            - generic [ref=f1e194]:
              - 'heading "Aborted transactions remembered: 0" [level=2] [ref=f1e195]'
              - paragraph [ref=f1e196]: An abort is remembered until the log start passes it; a commit is not. read_committed consumers are told these ranges and skip them; read_uncommitted consumers still see the records.
              - paragraph [ref=f1e197]: No aborted transaction is remembered on this partition.
      - generic [ref=f1e198]:
        - heading "Jump to a topic or group" [level=2] [ref=f1e199]
        - paragraph [ref=f1e200]: Type a name, then Enter
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