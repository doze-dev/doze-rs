# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: lab.spec.ts >> Lab: a Slow rule for one client slows its requests; the rule counts them; the sheet lists them with the lab tag
- Location: tests/lab.spec.ts:27:1

# Error details

```
Error: no console errors

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "pageerror: ResizeObserver loop completed with undelivered notifications.",
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
          - generic: "7"
        - listitem [ref=f1e33]:
          - link "Groups" [ref=f1e34]:
            - /url: /groups
          - generic: "3"
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
          - button "127.0.0.1:39392" [ref=f1e78]
          - generic [ref=f1e83]: Kafka 4 profile
          - link "Open" [ref=f1e84]:
            - /url: /access
      - main [ref=f1e86]:
        - generic [ref=f1e87]:
          - generic [ref=f1e88]:
            - navigation "Breadcrumb"
            - generic [ref=f1e89]:
              - generic [ref=f1e90]: Updated 05:42:50 UTC
              - combobox "Refresh every" [ref=f1e91]:
                - generic: "Refresh: Off"
              - button "Refresh" [ref=f1e92]
          - generic [ref=f1e93]:
            - generic [ref=f1e94]:
              - heading "Lab" [level=1] [ref=f1e95]
              - paragraph [ref=f1e96]: Rehearse a failure on purpose and watch what your app does. Each rehearsal is a visible rule with an end time, applied where connections are served, and every request it touches shows on the wire with a lab tag.
            - button "New rehearsal" [ref=f1e98]
        - generic [ref=f1e99]:
          - generic [ref=f1e100]:
            - generic [ref=f1e101]:
              - term [ref=f1e102]: Rules in force
              - definition [ref=f1e104]: 0 of 50
              - generic "0 switched on" [ref=f1e105]
            - generic [ref=f1e106]:
              - term [ref=f1e107]: Hits
              - definition [ref=f1e109]: "0"
              - generic "by the rules in force" [ref=f1e110]
            - generic [ref=f1e111]:
              - term [ref=f1e112]: Ended
              - definition [ref=f1e114]: "4"
              - generic "in the last hour" [ref=f1e115]
            - generic [ref=f1e116]:
              - term [ref=f1e117]: Done once
              - definition [ref=f1e119]: "0"
              - generic "drops, expiries, rebalances, cleans" [ref=f1e120]
          - region "Active" [ref=f1e121]:
            - generic [ref=f1e123]:
              - heading "Active" [level=2] [ref=f1e124]
              - paragraph [ref=f1e125]: What is bending your broker right now. Every rule ends by itself; removing it ends it sooner. A rule switched off matches nothing until switched on.
            - generic [ref=f1e126]:
              - table "Rules in force" [ref=f1e127]:
                - rowgroup [ref=f1e128]:
                  - row [ref=f1e129]:
                    - columnheader "On" [ref=f1e130]
                    - columnheader "Rule" [ref=f1e131]:
                      - button "Rule" [ref=f1e132]
                    - columnheader "Kind" [ref=f1e135]:
                      - button "Kind" [ref=f1e136]
                    - columnheader "Applies to" [ref=f1e139]:
                      - button "Applies to" [ref=f1e140]
                    - columnheader "Share" [ref=f1e143]:
                      - button "Share" [ref=f1e144]
                    - columnheader "Hits" [ref=f1e147]:
                      - button "Hits" [ref=f1e148]
                    - columnheader "Last hit" [ref=f1e151]:
                      - button "Last hit" [ref=f1e152]
                    - columnheader "Ends" [ref=f1e155]:
                      - button "Ends" [ref=f1e156]
                    - columnheader "Remove" [ref=f1e159]
                - rowgroup
              - generic [ref=f1e161]: No rule is in force. Start a rehearsal below.
          - region "Start a rehearsal" [ref=f1e162]:
            - generic [ref=f1e164]:
              - heading "Start a rehearsal" [level=2] [ref=f1e165]
              - paragraph [ref=f1e166]: Only what production code can honestly cause. Slow and Fail are rules that end on their own; the others happen once and leave a line of what they did.
            - generic [ref=f1e167]:
              - button "Slow requests rule Delay the answer to the requests you choose, for a while. Your producers and consumers see slow, not broken." [ref=f1e168]:
                - generic [ref=f1e169]:
                  - generic [ref=f1e173]: Slow requests
                  - generic [ref=f1e174]: rule
                - paragraph [ref=f1e175]: Delay the answer to the requests you choose, for a while. Your producers and consumers see slow, not broken.
              - button "Fail some requests rule Answer 1 in N of the requests you choose with an error Kafka has, such as REQUEST_TIMED_OUT, without asking the broker." [ref=f1e176]:
                - generic [ref=f1e177]:
                  - generic [ref=f1e180]: Fail some requests
                  - generic [ref=f1e181]: rule
                - paragraph [ref=f1e182]: Answer 1 in N of the requests you choose with an error Kafka has, such as REQUEST_TIMED_OUT, without asking the broker.
              - button "Drop a connection once Close one app's connections and see whether it reconnects on its own; or keep closing them for a few minutes." [ref=f1e183]:
                - generic [ref=f1e184]:
                  - generic [ref=f1e192]: Drop a connection
                  - generic [ref=f1e193]: once
                - paragraph [ref=f1e194]: Close one app's connections and see whether it reconnects on its own; or keep closing them for a few minutes.
              - button "Expire a member's session once Remove a group member as a missed heartbeat would, and read the story it leaves." [ref=f1e195]:
                - generic [ref=f1e196]:
                  - generic [ref=f1e202]: Expire a member's session
                  - generic [ref=f1e203]: once
                - paragraph [ref=f1e204]: Remove a group member as a missed heartbeat would, and read the story it leaves.
              - button "Force a rebalance once Start a rebalance of one group without waiting for a reason." [ref=f1e205]:
                - generic [ref=f1e206]:
                  - generic [ref=f1e212]: Force a rebalance
                  - generic [ref=f1e213]: once
                - paragraph [ref=f1e214]: Start a rebalance of one group without waiting for a reason.
              - button "Run the cleaner now once Clean one compacted partition this minute instead of at the cleaner's next look." [ref=f1e215]:
                - generic [ref=f1e216]:
                  - generic [ref=f1e222]: Run the cleaner now
                  - generic [ref=f1e223]: once
                - paragraph [ref=f1e224]: Clean one compacted partition this minute instead of at the cleaner's next look.
          - region "Ended" [ref=f1e225]:
            - generic [ref=f1e226]:
              - generic [ref=f1e227]:
                - heading "Ended" [level=2] [ref=f1e228]
                - paragraph [ref=f1e229]: Kept for the last hour, so you can look at what a rule did after it is gone.
              - generic [ref=f1e230]:
                - link "See the wire on Live" [ref=f1e231]:
                  - /url: /live
                - button "Clear history" [ref=f1e232]
            - table "Rules that ended" [ref=f1e234]:
              - rowgroup [ref=f1e235]:
                - row [ref=f1e236]:
                  - columnheader "Rule" [ref=f1e237]:
                    - button "Rule" [ref=f1e238]
                  - columnheader "Kind" [ref=f1e241]:
                    - button "Kind" [ref=f1e242]
                  - columnheader "How it ended" [ref=f1e245]:
                    - button "How it ended" [ref=f1e246]
                  - columnheader "Hits" [ref=f1e249]:
                    - button "Hits" [ref=f1e250]
                  - columnheader "From – to" [ref=f1e253]:
                    - button "From – to" [ref=f1e254]
              - rowgroup [ref=f1e257]:
                - row [ref=f1e258]:
                  - cell [ref=f1e259]:
                    - button "Slow produce to orders" [ref=f1e260]
                  - cell "slow" [ref=f1e261]
                  - cell "Removed" [ref=f1e263]
                  - cell "4" [ref=f1e264]
                  - cell "05:42 – 05:42" [ref=f1e265]
                - row [ref=f1e266]:
                  - cell [ref=f1e267]:
                    - button "Slow produce to orders" [ref=f1e268]
                  - cell "slow" [ref=f1e269]
                  - cell "Removed" [ref=f1e271]
                  - cell "4" [ref=f1e272]
                  - cell "05:42 – 05:42" [ref=f1e273]
                - row [ref=f1e274]:
                  - cell [ref=f1e275]:
                    - button "Slow produce to orders" [ref=f1e276]
                  - cell "slow" [ref=f1e277]
                  - cell "Removed" [ref=f1e279]
                  - cell "4" [ref=f1e280]
                  - cell "05:42 – 05:42" [ref=f1e281]
                - row [ref=f1e282]:
                  - cell [ref=f1e283]:
                    - button "Slow produce to orders" [ref=f1e284]
                  - cell "slow" [ref=f1e285]
                  - cell "Removed" [ref=f1e287]
                  - cell "4" [ref=f1e288]
                  - cell "05:42 – 05:42" [ref=f1e289]
      - generic [ref=f1e290]:
        - heading "Jump to a topic or group" [level=2] [ref=f1e291]
        - paragraph [ref=f1e292]: Type a name, then Enter
  - region "Notifications alt+T":
    - list:
      - listitem [ref=f1e293]:
        - generic [ref=f1e298]:
          - generic [ref=f1e299]: Rule removed
          - generic [ref=f1e300]: Slow produce to orders
      - listitem [ref=f1e301]:
        - generic [ref=f1e306]: Rule switched on
      - listitem [ref=f1e308]:
        - generic [ref=f1e313]: Rule switched off
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