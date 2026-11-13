# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-config.spec.ts >> the two lags together: min above max is refused in Kafka's words, Edit again, then saved
- Location: tests/topic-config.spec.ts:101:1

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
                      - generic: "24"
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
                - heading [level=1]: r1b-cfgg-webkit-mwe8l9tf-5
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-cfgg-webkit-mwe8l9tf-5
              - text: Messages
            - link:
              - /url: /topics/r1b-cfgg-webkit-mwe8l9tf-5/log
              - text: Log
            - link:
              - /url: /topics/r1b-cfgg-webkit-mwe8l9tf-5/config
              - text: Config
            - link:
              - /url: /topics/r1b-cfgg-webkit-mwe8l9tf-5/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-cfgg-webkit-mwe8l9tf-5/producers
              - text: Producers
          - generic:
            - paragraph: 33 configs. 4 are set on this topic, 1 comes from a broker flag, 28 are Kafka's defaults. The cleaner reads them at every look, so a change counts from the next one, within 15 s.
            - search:
              - generic:
                - textbox:
                  - /placeholder: Filter by name or by what it does
              - radiogroup:
                - radio [checked]:
                  - text: All
                  - generic: "33"
                - radio:
                  - text: Changed
                  - generic: "5"
                - radio: Compaction
                - radio: Segments
            - generic:
              - paragraph: "Where a value comes from, first match wins: set on this topic (Dynamic topic), then set for this broker or all brokers (Dynamic broker), then a start flag that differs from Kafka's default (Static flag), then Kafka's default. Open Where from on any changed row to see every place."
            - region:
              - heading [level=2]:
                - text: Changed from Kafka's default
                - generic: "5"
              - paragraph: Set here or by a flag. These are the ones worth reading first.
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: cleanup.policy
                        - paragraph: This config designates the retention policy to use on log segments.
                        - paragraph: A change counts on the next request.
                        - generic:
                          - button: Where from
                          - button: Kafka's text
                      - generic:
                        - generic: compact
                        - generic: keep the latest record of each key
                      - generic: Dynamic topic
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: segment.bytes
                        - paragraph: This configuration controls the segment file size for the log.
                        - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                        - generic:
                          - button: Where from
                          - button: Kafka's text
                      - generic:
                        - generic: "134217728"
                        - generic: 128 MiB
                      - generic: Static flag
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: retention.ms
                        - paragraph: This configuration controls the maximum time we will retain a log before we will discard old log segments to free up space if we are using the "delete" retention policy.
                        - paragraph: A change counts at the next retention pass, within 5 m.
                        - generic:
                          - button: Where from
                          - button: Kafka's text
                      - generic:
                        - generic: "604800000"
                        - generic: 7 d; the flag is no time limit
                      - generic: Dynamic topic
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: max.compaction.lag.ms
                        - paragraph: The maximum time a message will remain ineligible for compaction in the log.
                        - paragraph: A change counts at the cleaner's next look, within 15 s.
                        - generic:
                          - button: Where from
                          - button: Kafka's text
                      - generic:
                        - generic: "60000"
                        - generic: 1 min
                      - generic: Dynamic topic
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: min.compaction.lag.ms
                        - paragraph: The minimum time a message will remain uncompacted in the log.
                        - paragraph: A change counts at the cleaner's next look, within 15 s.
                        - generic:
                          - button: Where from
                          - button: Kafka's text
                      - generic:
                        - generic: "1000"
                        - generic: 1 s
                      - generic: Dynamic topic
                      - generic:
                        - button: Edit
            - region:
              - heading [level=2]:
                - text: Cleanup and compaction
                - generic: "2"
              - paragraph: How the cleaner decides.
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: min.cleanable.dirty.ratio
                        - paragraph: This configuration controls how frequently the log compactor will attempt to clean the log (assuming log compaction is enabled).
                        - paragraph: A change counts at the cleaner's next look, within 15 s.
                        - generic:
                          - button: Kafka's text
                      - generic: "0.5"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: delete.retention.ms
                        - paragraph: The amount of time to retain delete tombstone markers for log compacted topics.
                        - paragraph: A change counts at the cleaner's next look, within 15 s.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "86400000"
                        - generic: 1 d
                      - generic: Default
                      - generic:
                        - button: Edit
            - region:
              - heading [level=2]:
                - text: Retention
                - generic: "1"
              - paragraph: "Not used by this topic: it only compacts."
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: retention.bytes
                        - paragraph: This configuration controls the maximum size a partition (which consists of log segments) can grow to before we will discard old log segments to free up space if we are using the "delete" retention policy.
                        - paragraph: A change counts at the next retention pass, within 5 m.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "-1"
                        - generic: no size limit
                      - generic: Default
                      - generic:
                        - button: Edit
            - region:
              - heading [level=2]:
                - text: Segments and index
                - generic: "6"
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: segment.jitter.ms
                        - paragraph: The maximum random jitter subtracted from the scheduled segment roll time to avoid thundering herds of segment rolling
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic: "0"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: file.delete.delay.ms
                        - paragraph: The time to wait before deleting a file from the filesystem
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic:
                        - generic: "60000"
                        - generic: 1 min
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: preallocate
                        - paragraph: True if we should preallocate the file on disk when creating a new log segment.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic: "false"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: index.interval.bytes
                        - paragraph: This setting controls how frequently Kafka adds entries to its offset index and, conditionally, to its time index.
                        - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "4096"
                        - generic: 4 KiB
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: segment.ms
                        - paragraph: This configuration controls the period of time after which Kafka will force the log to roll even if the segment file isn't full to ensure that retention can delete or compact old data.
                        - paragraph: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                      - generic:
                        - generic: "604800000"
                        - generic: 7 d
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: segment.index.bytes
                        - paragraph: This configuration controls the size of the index that maps offsets to file positions.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "10485760"
                        - generic: 10 MiB
                      - generic: Default
                      - generic:
                        - button: Edit
            - region:
              - heading [level=2]:
                - text: Records
                - generic: "8"
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: compression.type
                        - paragraph: Specify the final compression type for a given topic.
                        - paragraph: A change counts on the next request.
                        - generic:
                          - button: Kafka's text
                      - generic: producer
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: compression.lz4.level
                        - paragraph: The compression level to use if compression.type is set to lz4.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic: "9"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: compression.gzip.level
                        - paragraph: The compression level to use if compression.type is set to gzip.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic: "-1"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: compression.zstd.level
                        - paragraph: The compression level to use if compression.type is set to zstd.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                      - generic: "3"
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: max.message.bytes
                        - paragraph: The largest record batch size allowed by Kafka (after compression if compression is enabled).
                        - paragraph: A change counts on the next request.
                      - generic:
                        - generic: "1048588"
                        - generic: about 1 MiB
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: message.timestamp.type
                        - paragraph: Define whether the timestamp in the message is message create time or log append time.
                        - paragraph: A change counts on the next request.
                      - generic: CreateTime
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: message.timestamp.after.max.ms
                        - paragraph: This configuration sets the allowable timestamp difference between the message timestamp and the broker's timestamp.
                        - paragraph: A change counts on the next request.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "3600000"
                        - generic: 1 h
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: message.timestamp.before.max.ms
                        - paragraph: This configuration sets the allowable timestamp difference between the broker's timestamp and the message timestamp.
                        - paragraph: A change counts on the next request.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "9223372036854775807"
                        - generic: no limit
                      - generic: Default
                      - generic:
                        - button: Edit
            - region:
              - heading [level=2]:
                - text: Durability
                - generic: "2"
              - generic:
                - generic [aria-hidden]:
                  - generic: Config
                  - generic: Value
                  - generic: Source
                - list:
                  - listitem:
                    - generic:
                      - generic:
                        - generic: flush.ms
                        - paragraph: This setting allows specifying a time interval at which we will force an fsync of data written to the log.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "9223372036854775807"
                        - generic: no limit
                      - generic: Default
                      - generic:
                        - button: Edit
                  - listitem:
                    - generic:
                      - generic:
                        - generic: flush.messages
                        - paragraph: This setting allows specifying an interval at which we will force an fsync of data written to the log.
                        - paragraph: This broker keeps the value and shows it; it does nothing with it.
                        - generic:
                          - button: Kafka's text
                      - generic:
                        - generic: "9223372036854775807"
                        - generic: no limit
                      - generic: Default
                      - generic:
                        - button: Edit
            - generic:
              - generic:
                - generic:
                  - heading [level=2]:
                    - text: Replication and tiered storage
                    - generic: "9"
                  - paragraph: "Configs Kafka lists that this broker has no use for: one broker, no replicas, no tiered storage."
                - button: Show 9
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Edit compaction lag" [active] [ref=e2]:
    - generic [ref=e3]:
      - heading "Edit compaction lag" [level=2] [ref=e4]
      - paragraph [ref=e5]: Check asks the broker with validate_only and changes nothing; Save sends IncrementalAlterConfigs.
    - generic [ref=e7]:
      - status [ref=e8]:
        - generic [ref=e12]:
          - text: Saved. IncrementalAlterConfigs answered
          - code [ref=e13]: "0"
          - text: for
          - code [ref=e14]: r1b-cfgg-webkit-mwe8l9tf-5
          - text: . min.compaction.lag.ms and max.compaction.lag.ms are now 1000 and 60000, set on this topic. The other 31 configs are unchanged; the cleaner sees the new value at its next look, within 15 s.
      - generic [ref=e15]:
        - generic [ref=e16]:
          - generic [ref=e17]: Config
          - generic [ref=e18]: Before
          - generic [ref=e19]: Now
        - generic [ref=e20]:
          - code [ref=e21]: min.compaction.lag.ms
          - generic [ref=e22]:
            - code [ref=e23]: "0"
            - text: Default
          - generic [ref=e24]:
            - code [ref=e25]: "1000"
            - text: Dynamic topic
        - generic [ref=e26]:
          - code [ref=e27]: max.compaction.lag.ms
          - generic [ref=e28]:
            - code [ref=e29]: "9223372036854775807"
            - text: Default
          - generic [ref=e30]:
            - code [ref=e31]: "60000"
            - text: Dynamic topic
      - generic [ref=e32]:
        - heading "What it does to this log" [level=3] [ref=e33]
        - paragraph [ref=e34]: A change counts at the cleaner's next look, within 15 s.
        - paragraph [ref=e35]: A change does not rewrite anything. The cleaner's next look reads it. (within 15 s)
      - button "Done" [ref=e37]
    - button "Close" [ref=e38]
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