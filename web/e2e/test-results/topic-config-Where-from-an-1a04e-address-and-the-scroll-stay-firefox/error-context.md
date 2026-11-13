# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-config.spec.ts >> Where from and Kafka's text open in place: the address and the scroll stay
- Location: tests/topic-config.spec.ts:40:1

# Error details

```
TimeoutError: click: Timeout 8000ms exceeded.
Call log:
  - waiting for locator('[data-testid=config-row][data-key="segment.ms"]').first().getByRole('button', { name: 'Kafka\'s text' })

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
          - generic: "5"
        - listitem [ref=e35]:
          - link "Groups" [ref=e36] [cursor=pointer]:
            - /url: /groups
          - generic: "1"
        - listitem [ref=e43]:
          - link "Access" [ref=e44] [cursor=pointer]:
            - /url: /access
        - listitem [ref=e48]:
          - link "Lab" [ref=e49] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=e55]:
          - link "Settings" [ref=e56] [cursor=pointer]:
            - /url: /settings
      - list [ref=e62]:
        - listitem [ref=e63]:
          - link "Classic console" [ref=e64] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=e70]
    - main [ref=e71]:
      - generic [ref=e72]:
        - button "Toggle Sidebar" [ref=e73]
        - button "Jump to a topic or group ⌘K" [ref=e75]:
          - generic [ref=e79]: Jump to a topic or group
          - generic [ref=e80]: ⌘K
        - generic [ref=e81]:
          - button "127.0.0.1:39092" [ref=e82]
          - generic [ref=e87]: Kafka 4 profile
          - link "Open" [ref=e88] [cursor=pointer]:
            - /url: /access
      - main [ref=e90]:
        - generic [ref=e91]:
          - navigation "Breadcrumb" [ref=e93]:
            - list [ref=e94]:
              - listitem [ref=e95]:
                - link "Topics" [ref=e96] [cursor=pointer]:
                  - /url: /topics
          - generic [ref=e97]:
            - heading "r1b-cfgp-firefox-mwe86d6c-1" [level=1] [ref=e99]
            - generic [ref=e100]:
              - button "Add partitions" [ref=e101]
              - button "Delete records" [ref=e102]
              - button "Produce" [ref=e103]
              - button "Delete topic" [ref=e104]
        - navigation "Topic" [ref=e105]:
          - link "Messages" [ref=e106] [cursor=pointer]:
            - /url: /topics/r1b-cfgp-firefox-mwe86d6c-1
          - link "Log" [ref=e107] [cursor=pointer]:
            - /url: /topics/r1b-cfgp-firefox-mwe86d6c-1/log
          - link "Config" [ref=e108] [cursor=pointer]:
            - /url: /topics/r1b-cfgp-firefox-mwe86d6c-1/config
          - link "Consumers" [ref=e109] [cursor=pointer]:
            - /url: /topics/r1b-cfgp-firefox-mwe86d6c-1/consumers
          - link "Producers" [ref=e110] [cursor=pointer]:
            - /url: /topics/r1b-cfgp-firefox-mwe86d6c-1/producers
        - generic [ref=e111]:
          - paragraph [ref=e112]: 33 configs. 2 are set on this topic, 1 comes from a broker flag, 30 are Kafka's defaults. The cleaner reads them at every look, so a change counts from the next one, within 15 s.
          - search [ref=e113]:
            - textbox "Filter configs" [ref=e115]:
              - /placeholder: Filter by name or by what it does
            - radiogroup "Show" [ref=e116]:
              - radio "All 33" [checked] [ref=e117]:
                - text: All
                - generic [ref=e118]: "33"
              - radio "Changed 3" [ref=e119]:
                - text: Changed
                - generic [ref=e120]: "3"
              - radio "Compaction" [ref=e121]
              - radio "Segments" [ref=e122]
          - paragraph [ref=e128]: "Where a value comes from, first match wins: set on this topic (Dynamic topic), then set for this broker or all brokers (Dynamic broker), then a start flag that differs from Kafka's default (Static flag), then Kafka's default. Open Where from on any changed row to see every place."
          - region "Changed from Kafka's default 3" [ref=e129]:
            - heading "Changed from Kafka's default 3" [level=2] [ref=e130]:
              - text: Changed from Kafka's default
              - generic [ref=e131]: "3"
            - paragraph [ref=e132]: Set here or by a flag. These are the ones worth reading first.
            - generic [ref=e133]:
              - generic [aria-hidden] [ref=e134]:
                - generic [ref=e135]: Config
                - generic [ref=e136]: Value
                - generic [ref=e137]: Source
              - list [ref=e139]:
                - listitem [ref=e140]:
                  - generic [ref=e141]:
                    - generic [ref=e142]:
                      - generic [ref=e143]: cleanup.policy
                      - paragraph [ref=e144]: This config designates the retention policy to use on log segments.
                      - paragraph [ref=e145]: A change counts on the next request.
                      - generic [ref=e146]:
                        - button "Where from" [ref=e147]
                        - button "Kafka's text" [ref=e150]
                    - generic [ref=e153]:
                      - generic [ref=e154]: delete
                      - generic [ref=e155]: discard old segments
                    - generic [ref=e156]: Dynamic topic
                    - button "Actions for cleanup.policy" [ref=e159]: Edit
                - listitem [ref=e160]:
                  - generic [ref=e161]:
                    - generic [ref=e162]:
                      - generic [ref=e163]: segment.bytes
                      - paragraph [ref=e164]: This configuration controls the segment file size for the log.
                      - paragraph [ref=e165]: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                      - generic [ref=e166]:
                        - button "Where from" [ref=e167]
                        - button "Kafka's text" [ref=e170]
                    - generic [ref=e173]:
                      - generic [ref=e174]: "134217728"
                      - generic [ref=e175]: 128 MiB
                    - generic [ref=e176]: Static flag
                    - button "Edit segment.bytes" [ref=e179]: Edit
                - listitem [ref=e180]:
                  - generic [ref=e181]:
                    - generic [ref=e182]:
                      - generic [ref=e183]: retention.ms
                      - paragraph [ref=e184]: This configuration controls the maximum time we will retain a log before we will discard old log segments to free up space if we are using the "delete" retention policy.
                      - paragraph [ref=e185]: A change counts at the next retention pass, within 5 m.
                      - generic [ref=e186]:
                        - button "Where from" [ref=e187]
                        - button "Kafka's text" [ref=e190]
                    - generic [ref=e193]:
                      - generic [ref=e194]: "3600000"
                      - generic [ref=e195]: 1 h; the flag is no time limit
                    - generic [ref=e196]: Dynamic topic
                    - button "Actions for retention.ms" [ref=e199]: Edit
          - region "Cleanup and compaction 4" [ref=e200]:
            - heading "Cleanup and compaction 4" [level=2] [ref=e201]:
              - text: Cleanup and compaction
              - generic [ref=e202]: "4"
            - paragraph [ref=e203]: How the cleaner decides.
            - generic [ref=e204]:
              - generic [aria-hidden] [ref=e205]:
                - generic [ref=e206]: Config
                - generic [ref=e207]: Value
                - generic [ref=e208]: Source
              - list [ref=e210]:
                - listitem [ref=e211]:
                  - generic [ref=e212]:
                    - generic [ref=e213]:
                      - generic [ref=e214]: max.compaction.lag.ms
                      - paragraph [ref=e215]: The maximum time a message will remain ineligible for compaction in the log.
                      - paragraph [ref=e216]: A change counts at the cleaner's next look, within 15 s.
                      - button "Kafka's text" [ref=e218]
                    - generic [ref=e221]:
                      - generic [ref=e222]: "9223372036854775807"
                      - generic [ref=e223]: no limit
                    - generic [ref=e224]: Default
                    - button "Edit max.compaction.lag.ms" [ref=e227]: Edit
                - listitem [ref=e228]:
                  - generic [ref=e229]:
                    - generic [ref=e230]:
                      - generic [ref=e231]: min.compaction.lag.ms
                      - paragraph [ref=e232]: The minimum time a message will remain uncompacted in the log.
                      - paragraph [ref=e233]: A change counts at the cleaner's next look, within 15 s.
                      - button "Kafka's text" [ref=e235]
                    - generic [ref=e238]:
                      - generic [ref=e239]: "0"
                      - generic [ref=e240]: no waiting
                    - generic [ref=e241]: Default
                    - button "Edit min.compaction.lag.ms" [ref=e244]: Edit
                - listitem [ref=e245]:
                  - generic [ref=e246]:
                    - generic [ref=e247]:
                      - generic [ref=e248]: min.cleanable.dirty.ratio
                      - paragraph [ref=e249]: This configuration controls how frequently the log compactor will attempt to clean the log (assuming log compaction is enabled).
                      - paragraph [ref=e250]: A change counts at the cleaner's next look, within 15 s.
                      - button "Kafka's text" [ref=e252]
                    - generic [ref=e255]: "0.5"
                    - generic [ref=e257]: Default
                    - button "Edit min.cleanable.dirty.ratio" [ref=e260]: Edit
                - listitem [ref=e261]:
                  - generic [ref=e262]:
                    - generic [ref=e263]:
                      - generic [ref=e264]: delete.retention.ms
                      - paragraph [ref=e265]: The amount of time to retain delete tombstone markers for log compacted topics.
                      - paragraph [ref=e266]: A change counts at the cleaner's next look, within 15 s.
                      - button "Kafka's text" [ref=e268]
                    - generic [ref=e271]:
                      - generic [ref=e272]: "86400000"
                      - generic [ref=e273]: 1 d
                    - generic [ref=e274]: Default
                    - button "Edit delete.retention.ms" [ref=e277]: Edit
          - region "Retention 1" [ref=e278]:
            - heading "Retention 1" [level=2] [ref=e279]:
              - text: Retention
              - generic [ref=e280]: "1"
            - paragraph [ref=e281]: When old records are deleted by age or size.
            - generic [ref=e282]:
              - generic [aria-hidden] [ref=e283]:
                - generic [ref=e284]: Config
                - generic [ref=e285]: Value
                - generic [ref=e286]: Source
              - list [ref=e288]:
                - listitem [ref=e289]:
                  - generic [ref=e290]:
                    - generic [ref=e291]:
                      - generic [ref=e292]: retention.bytes
                      - paragraph [ref=e293]: This configuration controls the maximum size a partition (which consists of log segments) can grow to before we will discard old log segments to free up space if we are using the "delete" retention policy.
                      - paragraph [ref=e294]: A change counts at the next retention pass, within 5 m.
                      - button "Kafka's text" [ref=e296]
                    - generic [ref=e299]:
                      - generic [ref=e300]: "-1"
                      - generic [ref=e301]: no size limit
                    - generic [ref=e302]: Default
                    - button "Edit retention.bytes" [ref=e305]: Edit
          - region "Segments and index 6" [ref=e306]:
            - heading "Segments and index 6" [level=2] [ref=e307]:
              - text: Segments and index
              - generic [ref=e308]: "6"
            - generic [ref=e309]:
              - generic [aria-hidden] [ref=e310]:
                - generic [ref=e311]: Config
                - generic [ref=e312]: Value
                - generic [ref=e313]: Source
              - list [ref=e315]:
                - listitem [ref=e316]:
                  - generic [ref=e317]:
                    - generic [ref=e318]:
                      - generic [ref=e319]: segment.jitter.ms
                      - paragraph [ref=e320]: The maximum random jitter subtracted from the scheduled segment roll time to avoid thundering herds of segment rolling
                      - paragraph [ref=e321]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e322]: "0"
                    - generic [ref=e324]: Default
                    - button "Edit segment.jitter.ms" [ref=e327]: Edit
                - listitem [ref=e328]:
                  - generic [ref=e329]:
                    - generic [ref=e330]:
                      - generic [ref=e331]: file.delete.delay.ms
                      - paragraph [ref=e332]: The time to wait before deleting a file from the filesystem
                      - paragraph [ref=e333]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e334]:
                      - generic [ref=e335]: "60000"
                      - generic [ref=e336]: 1 min
                    - generic [ref=e337]: Default
                    - button "Edit file.delete.delay.ms" [ref=e340]: Edit
                - listitem [ref=e341]:
                  - generic [ref=e342]:
                    - generic [ref=e343]:
                      - generic [ref=e344]: preallocate
                      - paragraph [ref=e345]: True if we should preallocate the file on disk when creating a new log segment.
                      - paragraph [ref=e346]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e347]: "false"
                    - generic [ref=e349]: Default
                    - button "Edit preallocate" [ref=e352]: Edit
                - listitem [ref=e353]:
                  - generic [ref=e354]:
                    - generic [ref=e355]:
                      - generic [ref=e356]: index.interval.bytes
                      - paragraph [ref=e357]: This setting controls how frequently Kafka adds entries to its offset index and, conditionally, to its time index.
                      - paragraph [ref=e358]: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                      - button "Kafka's text" [ref=e360]
                    - generic [ref=e363]:
                      - generic [ref=e364]: "4096"
                      - generic [ref=e365]: 4 KiB
                    - generic [ref=e366]: Default
                    - button "Edit index.interval.bytes" [ref=e369]: Edit
                - listitem [ref=e370]:
                  - generic [ref=e371]:
                    - generic [ref=e372]:
                      - generic [ref=e373]: segment.ms
                      - paragraph [ref=e374]: This configuration controls the period of time after which Kafka will force the log to roll even if the segment file isn't full to ensure that retention can delete or compact old data.
                      - paragraph [ref=e375]: A change counts from the next append, by the open partition's writer; existing segments keep their size.
                    - generic [ref=e376]:
                      - generic [ref=e377]: "604800000"
                      - generic [ref=e378]: 7 d
                    - generic [ref=e379]: Default
                    - button "Edit segment.ms" [ref=e382]: Edit
                - listitem [ref=e383]:
                  - generic [ref=e384]:
                    - generic [ref=e385]:
                      - generic [ref=e386]: segment.index.bytes
                      - paragraph [ref=e387]: This configuration controls the size of the index that maps offsets to file positions.
                      - paragraph [ref=e388]: This broker keeps the value and shows it; it does nothing with it.
                      - button "Kafka's text" [ref=e390]
                    - generic [ref=e393]:
                      - generic [ref=e394]: "10485760"
                      - generic [ref=e395]: 10 MiB
                    - generic [ref=e396]: Default
                    - button "Edit segment.index.bytes" [ref=e399]: Edit
          - region "Records 8" [ref=e400]:
            - heading "Records 8" [level=2] [ref=e401]:
              - text: Records
              - generic [ref=e402]: "8"
            - generic [ref=e403]:
              - generic [aria-hidden] [ref=e404]:
                - generic [ref=e405]: Config
                - generic [ref=e406]: Value
                - generic [ref=e407]: Source
              - list [ref=e409]:
                - listitem [ref=e410]:
                  - generic [ref=e411]:
                    - generic [ref=e412]:
                      - generic [ref=e413]: compression.type
                      - paragraph [ref=e414]: Specify the final compression type for a given topic.
                      - paragraph [ref=e415]: A change counts on the next request.
                      - button "Kafka's text" [ref=e417]
                    - generic [ref=e420]: producer
                    - generic [ref=e422]: Default
                    - button "Edit compression.type" [ref=e425]: Edit
                - listitem [ref=e426]:
                  - generic [ref=e427]:
                    - generic [ref=e428]:
                      - generic [ref=e429]: compression.lz4.level
                      - paragraph [ref=e430]: The compression level to use if compression.type is set to lz4.
                      - paragraph [ref=e431]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e432]: "9"
                    - generic [ref=e434]: Default
                    - button "Edit compression.lz4.level" [ref=e437]: Edit
                - listitem [ref=e438]:
                  - generic [ref=e439]:
                    - generic [ref=e440]:
                      - generic [ref=e441]: compression.gzip.level
                      - paragraph [ref=e442]: The compression level to use if compression.type is set to gzip.
                      - paragraph [ref=e443]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e444]: "-1"
                    - generic [ref=e446]: Default
                    - button "Edit compression.gzip.level" [ref=e449]: Edit
                - listitem [ref=e450]:
                  - generic [ref=e451]:
                    - generic [ref=e452]:
                      - generic [ref=e453]: compression.zstd.level
                      - paragraph [ref=e454]: The compression level to use if compression.type is set to zstd.
                      - paragraph [ref=e455]: This broker keeps the value and shows it; it does nothing with it.
                    - generic [ref=e456]: "3"
                    - generic [ref=e458]: Default
                    - button "Edit compression.zstd.level" [ref=e461]: Edit
                - listitem [ref=e462]:
                  - generic [ref=e463]:
                    - generic [ref=e464]:
                      - generic [ref=e465]: max.message.bytes
                      - paragraph [ref=e466]: The largest record batch size allowed by Kafka (after compression if compression is enabled).
                      - paragraph [ref=e467]: A change counts on the next request.
                    - generic [ref=e468]:
                      - generic [ref=e469]: "1048588"
                      - generic [ref=e470]: about 1 MiB
                    - generic [ref=e471]: Default
                    - button "Edit max.message.bytes" [ref=e474]: Edit
                - listitem [ref=e475]:
                  - generic [ref=e476]:
                    - generic [ref=e477]:
                      - generic [ref=e478]: message.timestamp.type
                      - paragraph [ref=e479]: Define whether the timestamp in the message is message create time or log append time.
                      - paragraph [ref=e480]: A change counts on the next request.
                    - generic [ref=e481]: CreateTime
                    - generic [ref=e483]: Default
                    - button "Edit message.timestamp.type" [ref=e486]: Edit
                - listitem [ref=e487]:
                  - generic [ref=e488]:
                    - generic [ref=e489]:
                      - generic [ref=e490]: message.timestamp.after.max.ms
                      - paragraph [ref=e491]: This configuration sets the allowable timestamp difference between the message timestamp and the broker's timestamp.
                      - paragraph [ref=e492]: A change counts on the next request.
                      - button "Kafka's text" [ref=e494]
                    - generic [ref=e497]:
                      - generic [ref=e498]: "3600000"
                      - generic [ref=e499]: 1 h
                    - generic [ref=e500]: Default
                    - button "Edit message.timestamp.after.max.ms" [ref=e503]: Edit
                - listitem [ref=e504]:
                  - generic [ref=e505]:
                    - generic [ref=e506]:
                      - generic [ref=e507]: message.timestamp.before.max.ms
                      - paragraph [ref=e508]: This configuration sets the allowable timestamp difference between the broker's timestamp and the message timestamp.
                      - paragraph [ref=e509]: A change counts on the next request.
                      - button "Kafka's text" [ref=e511]
                    - generic [ref=e514]:
                      - generic [ref=e515]: "9223372036854775807"
                      - generic [ref=e516]: no limit
                    - generic [ref=e517]: Default
                    - button "Edit message.timestamp.before.max.ms" [ref=e520]: Edit
          - region "Durability 2" [ref=e521]:
            - heading "Durability 2" [level=2] [ref=e522]:
              - text: Durability
              - generic [ref=e523]: "2"
            - generic [ref=e524]:
              - generic [aria-hidden] [ref=e525]:
                - generic [ref=e526]: Config
                - generic [ref=e527]: Value
                - generic [ref=e528]: Source
              - list [ref=e530]:
                - listitem [ref=e531]:
                  - generic [ref=e532]:
                    - generic [ref=e533]:
                      - generic [ref=e534]: flush.ms
                      - paragraph [ref=e535]: This setting allows specifying a time interval at which we will force an fsync of data written to the log.
                      - paragraph [ref=e536]: This broker keeps the value and shows it; it does nothing with it.
                      - button "Kafka's text" [ref=e538]
                    - generic [ref=e541]:
                      - generic [ref=e542]: "9223372036854775807"
                      - generic [ref=e543]: no limit
                    - generic [ref=e544]: Default
                    - button "Edit flush.ms" [ref=e547]: Edit
                - listitem [ref=e548]:
                  - generic [ref=e549]:
                    - generic [ref=e550]:
                      - generic [ref=e551]: flush.messages
                      - paragraph [ref=e552]: This setting allows specifying an interval at which we will force an fsync of data written to the log.
                      - paragraph [ref=e553]: This broker keeps the value and shows it; it does nothing with it.
                      - button "Kafka's text" [ref=e555]
                    - generic [ref=e558]:
                      - generic [ref=e559]: "9223372036854775807"
                      - generic [ref=e560]: no limit
                    - generic [ref=e561]: Default
                    - button "Edit flush.messages" [ref=e564]: Edit
          - generic [ref=e566]:
            - generic [ref=e567]:
              - heading "Replication and tiered storage 9" [level=2] [ref=e568]:
                - text: Replication and tiered storage
                - generic [ref=e569]: "9"
              - paragraph [ref=e570]: "Configs Kafka lists that this broker has no use for: one broker, no replicas, no tiered storage."
            - button "Show 9" [ref=e571]
      - generic [ref=e572]:
        - heading "Jump to a topic or group" [level=2] [ref=e573]
        - paragraph [ref=e574]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  1   | // Slice R1b: a topic's Config tab: the filter and the chips in the address, the panels that open in place, and the
  2   | // dialogs that change a config (Check, Save, a list, the two lags, a refused value) and reset it.
  3   | import { expect, overflowX, test } from "./fixtures";
  4   | import { call, fresh, get, topic } from "./r1b-helpers";
  5   | 
  6   | const configUrl = (t: string, q = "") => `/topics/${encodeURIComponent(t)}/config${q}`;
  7   | const row = (page: import("@playwright/test").Page, key: string) => page.locator(`[data-testid=config-row][data-key="${key}"]`).first();
  8   | 
  9   | test("the chips and the filter are in the address, Back restores them, and nothing scrolls sideways", async ({ page, request, baseURL }, info) => {
  10  |   const name = fresh(info, "cfg");
  11  |   await topic(request, baseURL!, name, 1, "short");
  12  |   await page.goto(configUrl(name));
  13  |   await expect(page.getByTestId("config-sentence")).toContainText("configs.");
  14  |   await expect(page.getByTestId("section-changed")).toContainText("retention.ms");
  15  |   const chips = page.getByTestId("config-chips");
  16  |   await chips.getByRole("radio", { name: /Changed/ }).click();
  17  |   await expect(page).toHaveURL(/show=changed/);
  18  |   await expect(page.getByTestId("section-segments")).toHaveCount(0);
  19  |   await chips.getByRole("radio", { name: "Segments" }).click();
  20  |   await expect(page).toHaveURL(/show=segments/);
  21  |   await expect(row(page, "segment.bytes")).toBeVisible();
  22  |   await expect(page.getByTestId("section-cleanup")).toHaveCount(0);
  23  |   await page.goBack();
  24  |   await expect(page).toHaveURL(/show=changed/);
  25  |   await expect(chips.getByRole("radio", { name: /Changed/ })).toHaveAttribute("aria-checked", "true");
  26  |   await chips.getByRole("radio", { name: /All/ }).click();
  27  |   await expect(page).not.toHaveURL(/show=/);
  28  |   await page.getByTestId("config-filter").fill("dirty");
  29  |   await expect(page).toHaveURL(/q=dirty/);
  30  |   await expect(page.getByTestId("config-row")).toHaveCount(1);
  31  |   await expect(row(page, "min.cleanable.dirty.ratio")).toBeVisible();
  32  |   await page.getByRole("button", { name: "Clear the filter" }).click();
  33  |   await expect(page).not.toHaveURL(/q=/);
  34  |   for (const w of [1024, 1280, 1440, 1920]) {
  35  |     await page.setViewportSize({ width: w, height: 900 });
  36  |     expect(await overflowX(page), `overflow at ${w}`).toBe(0);
  37  |   }
  38  | });
  39  | 
  40  | test("Where from and Kafka's text open in place: the address and the scroll stay", async ({ page, request, baseURL }, info) => {
  41  |   const name = fresh(info, "cfgp");
  42  |   await topic(request, baseURL!, name, 1, "short");
  43  |   await page.goto(configUrl(name));
  44  |   const r = row(page, "segment.ms");
  45  |   await r.scrollIntoViewIfNeeded();
  46  |   const url = page.url();
  47  |   const y = await page.evaluate(() => window.scrollY);
> 48  |   await r.getByRole("button", { name: "Kafka's text" }).click();
      |                                                         ^ TimeoutError: click: Timeout 8000ms exceeded.
  49  |   await expect(r.getByTestId("kafka-text")).toBeVisible();
  50  |   await expect(r.getByTestId("kafka-text")).toContainText("Type");
  51  |   expect(page.url()).toBe(url);
  52  |   expect(Math.abs((await page.evaluate(() => window.scrollY)) - y)).toBeLessThan(2);
  53  |   await r.getByRole("button", { name: "Kafka's text" }).click();
  54  |   await expect(r.getByTestId("kafka-text")).toBeHidden();
  55  |   const ret = row(page, "retention.ms");
  56  |   await ret.getByRole("button", { name: "Where from" }).click();
  57  |   await expect(ret.getByTestId("where-from")).toContainText("Dynamic topic");
  58  |   await expect(ret.getByTestId("where-from")).toContainText("takes over");
  59  |   expect(page.url()).toBe(url);
  60  | });
  61  | 
  62  | test("edit a value: Check asks Kafka and changes nothing, Save changes it, and the tab shows it without a reload", async ({ page, request, baseURL }, info) => {
  63  |   const name = fresh(info, "cfge");
  64  |   await topic(request, baseURL!, name, 1, "short");
  65  |   await page.goto(configUrl(name));
  66  |   const r = row(page, "retention.ms");
  67  |   await r.getByTestId("row-menu").click();
  68  |   await page.getByRole("menuitem", { name: "Edit" }).click();
  69  |   await expect(page).toHaveURL(/edit=retention\.ms/);
  70  |   const d = page.getByTestId("dialog-config-edit");
  71  |   await expect(d.getByTestId("config-value")).toHaveValue("3600000");
  72  |   await d.getByTestId("config-value").fill("7200000");
  73  |   await expect(d.getByTestId("the-call")).toContainText("7200000");
  74  |   await d.getByTestId("config-check").click();
  75  |   await expect(d.getByTestId("config-checked")).toContainText("Kafka would take this");
  76  |   const still = await get(request, `/api/v1/topics/${name}/config`);
  77  |   expect(still.body.rows.find((x: { name: string }) => x.name === "retention.ms").value).toBe("3600000");
  78  |   await d.getByTestId("config-save").click();
  79  |   await expect(d.getByTestId("config-saved")).toBeVisible();
  80  |   await expect(d.getByTestId("saved-now")).toContainText("7200000");
  81  |   await d.getByTestId("saved-done").click();
  82  |   await expect(d).toBeHidden();
  83  |   await expect(page).not.toHaveURL(/edit=/);
  84  |   await expect(r.getByTestId("config-value")).toHaveText("7200000");
  85  | });
  86  | 
  87  | test("a list: append a policy", async ({ page, request, baseURL }, info) => {
  88  |   const name = fresh(info, "cfgl");
  89  |   await topic(request, baseURL!, name, 1, "event");
  90  |   await page.goto(configUrl(name, "?edit=cleanup.policy"));
  91  |   const d = page.getByTestId("dialog-config-edit");
  92  |   await expect(d.getByTestId("config-op").getByRole("radio", { name: /Append/ })).toHaveAttribute("aria-checked", "true");
  93  |   await d.getByTestId("config-value").fill("compact");
  94  |   await expect(d.getByTestId("the-call")).toContainText("APPEND");
  95  |   await d.getByTestId("config-save").click();
  96  |   await expect(d.getByTestId("saved-now")).toContainText("delete,compact");
  97  |   await page.keyboard.press("Escape");
  98  |   await expect(row(page, "cleanup.policy").getByTestId("config-value")).toHaveText("delete,compact");
  99  | });
  100 | 
  101 | test("the two lags together: min above max is refused in Kafka's words, Edit again, then saved", async ({ page, request, baseURL }, info) => {
  102 |   const name = fresh(info, "cfgg");
  103 |   await topic(request, baseURL!, name, 1, "compact");
  104 |   await page.goto(configUrl(name, "?edit=min.compaction.lag.ms"));
  105 |   const d = page.getByTestId("dialog-config-edit");
  106 |   await expect(d.getByRole("heading", { name: "Edit compaction lag" })).toBeVisible();
  107 |   await d.getByTestId("config-value").fill("50000");
  108 |   await d.getByTestId("config-other").fill("10000");
  109 |   await d.getByTestId("config-save").click();
  110 |   const refused = d.getByTestId("config-refused");
  111 |   await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG");
  112 |   const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.compaction.lag.ms", op: "set", value: "50000", other: "10000" });
  113 |   await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  114 |   await expect(refused.getByTestId("would-pass")).toBeVisible();
  115 |   await refused.getByTestId("edit-again").click();
  116 |   await d.getByTestId("config-value").fill("1000");
  117 |   await d.getByTestId("config-other").fill("60000");
  118 |   await d.getByTestId("config-save").click();
  119 |   await expect(d.getByTestId("saved-change")).toHaveCount(2);
  120 | });
  121 | 
  122 | test("a value Kafka will not take: refused in its own words, and nothing changed", async ({ page, request, baseURL }, info) => {
  123 |   const name = fresh(info, "cfgv");
  124 |   await topic(request, baseURL!, name, 1, "event");
  125 |   await page.goto(configUrl(name, "?edit=min.cleanable.dirty.ratio"));
  126 |   const d = page.getByTestId("dialog-config-edit");
  127 |   await d.getByTestId("config-value").fill("7");
  128 |   await d.getByTestId("config-check").click();
  129 |   const refused = d.getByTestId("config-refused");
  130 |   await expect(refused.getByTestId("refusal-code")).toContainText("INVALID_CONFIG (40)");
  131 |   const api = await call(request, baseURL!, `/api/v1/topics/${name}/config/edit`, { key: "min.cleanable.dirty.ratio", op: "set", value: "7", other: "", check: "1" });
  132 |   await expect(refused.getByTestId("refusal-message")).toHaveText(api.body.message);
  133 |   const still = await get(request, `/api/v1/topics/${name}/config`);
  134 |   expect(still.body.rows.find((x: { name: string }) => x.name === "min.cleanable.dirty.ratio").source).not.toBe("dynamic_topic");
  135 | });
  136 | 
  137 | test("reset: the topic follows the broker again", async ({ page, request, baseURL }, info) => {
  138 |   const name = fresh(info, "cfgr");
  139 |   await topic(request, baseURL!, name, 1, "short");
  140 |   await page.goto(configUrl(name));
  141 |   const r = row(page, "retention.ms");
  142 |   await r.getByTestId("row-menu").click();
  143 |   await page.getByRole("menuitem", { name: "Reset to the inherited value" }).click();
  144 |   await expect(page).toHaveURL(/reset=retention\.ms/);
  145 |   const d = page.getByTestId("dialog-config-reset");
  146 |   await expect(d.getByTestId("reset-now")).toContainText("3600000");
  147 |   await expect(d.getByTestId("reset-after")).toContainText("604800000");
  148 |   await d.getByTestId("reset-submit").click();
```