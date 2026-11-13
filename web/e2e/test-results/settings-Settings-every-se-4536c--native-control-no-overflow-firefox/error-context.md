# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: settings.spec.ts >> Settings: every section, figures as tiles, the five configs, no memory figure, no native control, no overflow
- Location: tests/settings.spec.ts:6:1

# Error details

```
Error: tables wider than their box at 1024

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 4

- Array []
+ Array [
+   "Dynamic broker configs",
+   "Intervals",
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
          - generic: "7"
        - listitem [ref=e35]:
          - link "Groups" [ref=e36] [cursor=pointer]:
            - /url: /groups
          - generic: "6"
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
          - button "127.0.0.1:39392" [ref=e82]
          - link "Open" [ref=e87] [cursor=pointer]:
            - /url: /access
      - main [ref=e89]:
        - generic [ref=e90]:
          - generic [ref=e91]:
            - navigation "Breadcrumb"
          - generic [ref=e93]:
            - heading "Settings" [level=1] [ref=e94]
            - paragraph [ref=e95]: This broker's own configuration, explained. Five settings can change while it runs; everything else was fixed when it started and is shown read only.
        - generic [ref=e96]:
          - generic [ref=e97]:
            - term [ref=e98]: Set at run time
            - definition [ref=e100]: 1 of 5
            - generic "dynamic broker configs" [ref=e101]
          - generic [ref=e102]:
            - term [ref=e103]: Running since
            - definition [ref=e105]: Nov 13 06:55 UTC
            - generic "UTC" [ref=e106]
          - generic [ref=e107]:
            - term [ref=e108]: Version profile
            - definition [ref=e110]: Kafka 4
            - generic "--kafka-version" [ref=e111]
        - navigation "Sections" [ref=e112]:
          - link "Broker configs" [ref=e113] [cursor=pointer]:
            - /url: "#configs"
          - link "Version profile" [ref=e114] [cursor=pointer]:
            - /url: "#profile"
          - link "Intervals" [ref=e115] [cursor=pointer]:
            - /url: "#intervals"
          - link "Weight" [ref=e116] [cursor=pointer]:
            - /url: "#weight"
          - link "Data" [ref=e117] [cursor=pointer]:
            - /url: "#data"
          - link "Flags" [ref=e118] [cursor=pointer]:
            - /url: "#flags"
        - region "Broker configs" [ref=e119]:
          - generic [ref=e121]:
            - heading "Broker configs" [level=2] [ref=e122]
            - paragraph [ref=e123]: "Source says where the value in force comes from: a dynamic setting first, then the flag it started with, then Kafka's own default. A topic that sets its own value keeps it."
          - table "Dynamic broker configs" [ref=e125]:
            - rowgroup [ref=e132]:
              - row [ref=e133]:
                - columnheader "Config" [ref=e134]:
                  - button "Config" [ref=e135]
                - columnheader "Value" [ref=e138]:
                  - button "Value" [ref=e139]
                - columnheader "Source" [ref=e142]:
                  - button "Source" [ref=e143]
                - columnheader "Takes effect" [ref=e146]:
                  - button "Takes effect" [ref=e147]
                - columnheader "Edit" [ref=e150]
            - rowgroup [ref=e152]:
              - row [ref=e153]:
                - cell "log.segment.bytes" [ref=e154]
                - cell "134217728128 MiB" [ref=e155]:
                  - generic "134217728 (128 MiB)" [ref=e156]: 134217728128 MiB
                - cell [ref=e157]:
                  - 'generic "Static broker: flag --segment-bytes; Kafka''s default is 1 GiB" [ref=e158]': Static broker
                - cell "At the next append of each open partition" [ref=e161]
                - cell [ref=e162]:
                  - button "Edit log.segment.bytes" [ref=e163]: Edit
              - row [ref=e164]:
                - cell "log.index.interval.bytes" [ref=e165]
                - cell "40964 KiB" [ref=e166]:
                  - generic "4096 (4 KiB)" [ref=e167]: 40964 KiB
                - cell [ref=e168]:
                  - 'generic "Default: Kafka''s default" [ref=e169]': Default
                - cell "At the next append of each open partition" [ref=e172]
                - cell [ref=e173]:
                  - button "Edit log.index.interval.bytes" [ref=e174]: Edit
              - row [ref=e175]:
                - cell "message.max.bytes" [ref=e176]
                - cell "10485881 MiB + 12 B" [ref=e177]:
                  - generic "1048588 (1 MiB + 12 B)" [ref=e178]: 10485881 MiB + 12 B
                - cell [ref=e179]:
                  - 'generic "Default: Kafka''s default" [ref=e180]': Default
                - cell "On the next request" [ref=e183]
                - cell [ref=e184]:
                  - button "Edit message.max.bytes" [ref=e185]: Edit
              - row [ref=e186]:
                - cell "log.retention.bytes" [ref=e187]
                - cell "-1no limit" [ref=e188]:
                  - generic "-1 (no limit)" [ref=e189]: "-1no limit"
                - cell [ref=e190]:
                  - 'generic "Default: Kafka''s default" [ref=e191]': Default
                - cell "At the next retention pass, every 5 m" [ref=e194]
                - cell [ref=e195]:
                  - button "Edit log.retention.bytes" [ref=e196]: Edit
              - row [ref=e197]:
                - cell "log.retention.ms" [ref=e198]
                - cell "2592000003 d" [ref=e199]:
                  - generic "259200000 (3 d)" [ref=e200]: 2592000003 d
                - cell [ref=e201]:
                  - 'generic "Dynamic default broker: set for all brokers" [ref=e202]': Dynamic default broker
                - cell "At the next retention pass, every 5 m" [ref=e205]
                - cell [ref=e206]:
                  - button "Edit log.retention.ms" [ref=e207]: Edit
          - paragraph [ref=e208]: "A change is kept on disk and is still there after a restart. It applies to all brokers or to this one (node 1). These are the only five: any other config is refused, in Kafka's words."
        - region "Version profile" [ref=e209]:
          - generic [ref=e211]:
            - heading "Version profile" [level=2] [ref=e212]
            - paragraph [ref=e213]: The Kafka the broker stands for, and the APIs a client is offered, each with the versions a real Kafka 4 broker offers. It is what ApiVersions answers. A client picks, for each API, the highest version both sides have.
          - generic [ref=e214]:
            - generic [ref=e215]:
              - term [ref=e216]: Profile
              - definition [ref=e218]: Kafka 4
              - generic "--kafka-version" [ref=e219]
            - generic [ref=e220]:
              - term [ref=e221]: APIs offered
              - definition [ref=e223]: "41"
              - generic "SASL off" [ref=e224]
            - generic [ref=e225]:
              - term [ref=e226]: Not offered
              - definition [ref=e228]: "2"
              - generic "of this Kafka's APIs" [ref=e229]
          - list [ref=e230]:
            - listitem [ref=e231]:
              - code [ref=e232]: lab-once-firefox
              - generic [ref=e233]: speaks
              - code [ref=e234]: Fetch v11
              - generic [ref=e235]: inside the range
            - listitem [ref=e237]:
              - code [ref=e238]: fulfilment
              - generic [ref=e239]: speaks
              - code [ref=e240]: Produce v7
              - generic [ref=e241]: inside the range
          - table "APIs offered" [ref=e244]:
            - rowgroup [ref=e249]:
              - row [ref=e250]:
                - columnheader "API" [ref=e251]:
                  - button "API" [ref=e252]
                - columnheader "Versions" [ref=e255]:
                  - button "Versions" [ref=e256]
                - columnheader "Family" [ref=e259]:
                  - button "Family" [ref=e260]
            - rowgroup [ref=e263]:
              - row [ref=e264]:
                - cell "Produce" [ref=e265]
                - cell "v3 to v9" [ref=e266]
                - cell "Produce and fetch" [ref=e267]
              - row [ref=e268]:
                - cell "Fetch" [ref=e269]
                - cell "v4 to v13" [ref=e270]
                - cell "Produce and fetch" [ref=e271]
              - row [ref=e272]:
                - cell "ListOffsets" [ref=e273]
                - cell "v1 to v7" [ref=e274]
                - cell "Produce and fetch" [ref=e275]
              - row [ref=e276]:
                - cell "DeleteRecords" [ref=e277]
                - cell "v0 to v2" [ref=e278]
                - cell "Produce and fetch" [ref=e279]
              - row [ref=e280]:
                - cell "ApiVersions" [ref=e281]
                - cell "v0 to v4" [ref=e282]
                - cell "Metadata and bootstrapping" [ref=e283]
              - row [ref=e284]:
                - cell "Metadata" [ref=e285]
                - cell "v0 to v12" [ref=e286]
                - cell "Metadata and bootstrapping" [ref=e287]
              - row [ref=e288]:
                - cell "FindCoordinator" [ref=e289]
                - cell "v0 to v4" [ref=e290]
                - cell "Metadata and bootstrapping" [ref=e291]
              - row [ref=e292]:
                - cell "DescribeCluster" [ref=e293]
                - cell "v0 to v2" [ref=e294]
                - cell "Metadata and bootstrapping" [ref=e295]
          - button "Show all 41 APIs" [ref=e296]
          - paragraph [ref=e298]: "Not offered: SaslHandshake, SaslAuthenticate (no handler yet)."
        - region "Intervals" [ref=e299]:
          - generic [ref=e301]:
            - heading "Intervals" [level=2] [ref=e302]
            - paragraph [ref=e303]: "How often the broker looks at things on its own. Read only: set when the broker starts. To change an interval, stop the broker and start it with the flag."
          - generic [ref=e304]:
            - generic [ref=e305]:
              - term [ref=e306]: Retention check
              - definition [ref=e308]: 5 m
              - generic "every" [ref=e309]
            - generic [ref=e310]:
              - term [ref=e311]: Cleaner
              - definition [ref=e313]: 15 s
              - generic "looks every" [ref=e314]
            - generic [ref=e315]:
              - term [ref=e316]: Transactions
              - definition [ref=e318]: 500 ms
              - generic "checked every; no flag sets it" [ref=e319]
            - generic [ref=e320]:
              - term [ref=e321]: Last cleaner round
              - definition [ref=e323]: none yet
              - generic "since the broker began" [ref=e324]
          - table "Intervals" [ref=e326]:
            - rowgroup [ref=e333]:
              - row [ref=e334]:
                - columnheader "Interval" [ref=e335]:
                  - button "Interval" [ref=e336]
                - columnheader "Value" [ref=e339]:
                  - button "Value" [ref=e340]
                - columnheader "Flag" [ref=e343]:
                  - button "Flag" [ref=e344]
                - columnheader "Default" [ref=e347]:
                  - button "Default" [ref=e348]
                - columnheader "What it comes to" [ref=e351]:
                  - button "What it comes to" [ref=e352]
            - rowgroup [ref=e355]:
              - row [ref=e356]:
                - cell "Retention check" [ref=e357]
                - cell "5m" [ref=e358]
                - cell "--log-retention-check-interval" [ref=e359]
                - cell "5m" [ref=e360]
                - cell "Must be above 0" [ref=e361]
              - row [ref=e362]:
                - cell "Cleaner look" [ref=e363]
                - cell "15s" [ref=e364]
                - cell "--log-cleaner-interval" [ref=e365]
                - cell "15s" [ref=e366]
                - cell "Must be above 0" [ref=e367]
              - row [ref=e368]:
                - cell "Flush after a write" [ref=e369]
                - cell "500ms" [ref=e370]
                - cell "--fsync-interval" [ref=e371]
                - cell "500ms" [ref=e372]
                - cell "acks=all flushes before it answers" [ref=e373]
              - row [ref=e374]:
                - cell "Offsets retention" [ref=e375]
                - cell "168h" [ref=e376]
                - cell "--offsets-retention" [ref=e377]
                - cell "168h" [ref=e378]
                - cell "kept for 7 d; payments-workers' expire in 6 d 23 h" [ref=e379]
              - row [ref=e380]:
                - cell "First rebalance wait" [ref=e381]
                - cell "200ms" [ref=e382]
                - cell "--group-initial-rebalance-delay" [ref=e383]
                - cell "200ms" [ref=e384]
                - cell "0 does not wait" [ref=e385]
              - row [ref=e386]:
                - cell "Consumer session" [ref=e387]
                - cell "45s" [ref=e388]
                - cell "--consumer-session-timeout" [ref=e389]
                - cell "45s" [ref=e390]
                - cell "Must be above 0" [ref=e391]
        - region "Weight" [ref=e392]:
          - generic [ref=e394]:
            - heading "Weight" [level=2] [ref=e395]
            - paragraph [ref=e396]: "What this instance costs the machine on disk and in open partitions. Weight is a feature: the binary's size and its packages are measured in the tests and may only grow with a written reason."
          - generic [ref=e397]:
            - generic [ref=e398]:
              - term [ref=e399]: Binary
              - definition [ref=e401]: 6.3 MB
              - generic "the broker program, console included" [ref=e402]
            - generic [ref=e403]:
              - term [ref=e404]: Open partitions
              - definition [ref=e406]: 16 of 512
              - generic "--max-open-partitions 512" [ref=e407]
            - generic [ref=e408]:
              - term [ref=e409]: Partitions
              - definition [ref=e411]: "17"
              - generic "in 7 topics" [ref=e412]
        - region "Data" [ref=e413]:
          - generic [ref=e415]:
            - heading "Data" [level=2] [ref=e416]
            - paragraph [ref=e417]: Everything this broker keeps lives in one directory. Stop it cleanly and the directory can be copied, moved or deleted as a whole.
          - generic [ref=e418]:
            - generic [ref=e419]:
              - generic [ref=e420]: The data directory
              - generic [ref=e422]:
                - generic [ref=e423]:
                  - code [ref=e424]: /private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/r4data2
                  - button "Copy path" [ref=e425]
                - generic [ref=e427]:
                  - generic [ref=e428]:
                    - term [ref=e429]: Set by
                    - definition [ref=e430]:
                      - code [ref=e432]: "--data-dir"
                      - generic [ref=e433]: given at start
                  - generic [ref=e434]:
                    - term [ref=e435]: Size
                    - definition [ref=e436]:
                      - generic [ref=e437]: 2.9 MiB
                      - generic [ref=e438]: 35 files
                  - generic [ref=e439]:
                    - term [ref=e440]: Partitions on disk
                    - definition [ref=e441]:
                      - generic [ref=e442]: "17"
                      - generic [ref=e443]: in 7 topics
                  - generic [ref=e444]:
                    - term [ref=e445]: Last stop
                    - definition [ref=e446]:
                      - generic [ref=e447]: Clean
                      - generic [ref=e450]: the stop was recorded, so every partition was trusted at start
            - generic [ref=e451]:
              - generic [ref=e452]: What a restart does
              - generic [ref=e454]:
                - paragraph [ref=e455]: "SIGTERM or Ctrl-C is a clean stop: every open partition is flushed and closed and the stop is recorded, while a kill is not, and the next start distrusts every partition and the first flush covers what it reads back (nothing acknowledged is lost)."
                - paragraph [ref=e456]: Your data is where it was, including topics, records, configs, committed offsets, producer ids and open transactions, while connections, group members, the wire's rows and Lab rules start again from nothing. Stored ACLs and the users made over the API stay; users from flags are re-applied.
        - region "Flags" [ref=e457]:
          - generic [ref=e458]:
            - generic [ref=e459]:
              - heading "Flags" [level=2] [ref=e460]
              - paragraph [ref=e461]: "What the broker was started with, each with its default. Read only: set at start."
            - radiogroup "Which flags" [ref=e463]:
              - radio "Differ from default · 3" [checked] [ref=e464]
              - radio "All · 39" [ref=e465]
          - table "Flags" [ref=e467]:
            - rowgroup [ref=e473]:
              - row [ref=e474]:
                - columnheader "Flag" [ref=e475]:
                  - button "Flag" [ref=e476]
                - columnheader "Value" [ref=e479]:
                  - button "Value" [ref=e480]
                - columnheader "Default" [ref=e483]:
                  - button "Default" [ref=e484]
                - columnheader "What it does" [ref=e487]:
                  - button "What it does" [ref=e488]
            - rowgroup [ref=e491]:
              - row [ref=e492]:
                - cell "--listen" [ref=e493]
                - cell "127.0.0.1:39392" [ref=e494]
                - cell "127.0.0.1:9092" [ref=e495]
                - cell "set Serve on this host:port" [ref=e496]:
                  - generic "Serve on this host:port" [ref=e497]:
                    - generic [ref=e498]: set
                    - text: Serve on this host:port
              - row [ref=e499]:
                - cell "--data-dir" [ref=e500]
                - cell "/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/r4data2" [ref=e501]
                - cell "./data" [ref=e502]
                - cell "set Directory for the metadata database and the partition logs" [ref=e503]:
                  - generic "Directory for the metadata database and the partition logs" [ref=e504]:
                    - generic [ref=e505]: set
                    - text: Directory for the metadata database and the partition logs
              - row [ref=e506]:
                - cell "--console-addr" [ref=e507]
                - cell "127.0.0.1:39394" [ref=e508]
                - cell "127.0.0.1:9093" [ref=e509]
                - 'cell "set Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)" [ref=e510]':
                  - 'generic "Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)" [ref=e511]':
                    - generic [ref=e512]: set
                    - text: "Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)"
          - paragraph [ref=e513]:
            - generic [ref=e518]: "A flag that carries a secret says how many were given and never what. A broker that would be open while looking closed refuses to start (exit code 2): with SASL and nobody who could log in, with a certificate it cannot read, or with the console beyond loopback while SASL or TLS is on."
      - generic [ref=e519]:
        - heading "Jump to a topic or group" [level=2] [ref=e520]
        - paragraph [ref=e521]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  1  | // Helpers of the Settings, Access and Lab specs (slice R4): the broker's Kafka address, a client with traffic, the API with the
  2  | // console's proof, and the checks every page of the slice must pass.
  3  | import type { APIRequestContext, Page } from "@playwright/test";
  4  | import { Kafka, logLevel, type Producer } from "kafkajs";
  5  | import { expect, overflowX } from "./fixtures";
  6  | 
  7  | /** The Kafka address the console serves (the top bar's), so a spec can make traffic against a seeded or a demo broker. */
  8  | export async function kafkaAddress(request: APIRequestContext): Promise<string> {
  9  |   const res = await request.get("/api/v1/shell", { headers: { "X-Console": "1" } });
  10 |   return (await res.json()).address as string;
  11 | }
  12 | 
  13 | /** A POST to the console's API as its own page sends it. */
  14 | export async function apiPost(request: APIRequestContext, baseURL: string, path: string, body: unknown) {
  15 |   const res = await request.post(path, { headers: { "X-Console": "1", Origin: new URL(baseURL).origin, "Content-Type": "application/json" }, data: JSON.stringify(body) });
  16 |   return { status: res.status(), body: await res.json() };
  17 | }
  18 | 
  19 | export async function apiGet(request: APIRequestContext, path: string) {
  20 |   const res = await request.get(path, { headers: { "X-Console": "1" } });
  21 |   return res.json();
  22 | }
  23 | 
  24 | /** A producer that keeps writing to `topic` every `ms` until stopped. */
  25 | export async function traffic(address: string, clientId: string, topic = "orders", ms = 150) {
  26 |   const kafka = new Kafka({ clientId, brokers: [address], logLevel: logLevel.NOTHING, retry: { retries: 0 } });
  27 |   const producer: Producer = kafka.producer({ allowAutoTopicCreation: false, retry: { retries: 0 } });
  28 |   await producer.connect();
  29 |   let on = true;
  30 |   let sent = 0;
  31 |   const errors: string[] = [];
  32 |   const loop = (async () => {
  33 |     while (on) {
  34 |       try {
  35 |         await producer.send({ topic, acks: 1, messages: [{ key: `k${sent}`, value: `lab ${sent}` }] });
  36 |         sent++;
  37 |       } catch (e) {
  38 |         errors.push((e as { type?: string }).type ?? String(e));
  39 |       }
  40 |       await new Promise((r) => setTimeout(r, ms));
  41 |     }
  42 |   })();
  43 |   return {
  44 |     kafka,
  45 |     producer,
  46 |     errors,
  47 |     sent: () => sent,
  48 |     async stop() {
  49 |       on = false;
  50 |       await loop;
  51 |       await producer.disconnect().catch(() => {});
  52 |     },
  53 |   };
  54 | }
  55 | 
  56 | /** No browser-default control: a native select may exist only as Radix's hidden one (aria-hidden, a pixel wide). */
  57 | export async function noNativeSelect(page: Page) {
  58 |   const shown = await page.evaluate(() =>
  59 |     [...document.querySelectorAll("select, input[type=date], input[type=number], input[type=time], input[type=datetime-local]")].filter((el) => {
  60 |       const r = el.getBoundingClientRect();
  61 |       return el.getAttribute("aria-hidden") !== "true" && r.width > 1 && r.height > 1;
  62 |     }).length,
  63 |   );
  64 |   expect(shown, "no native select, date or number widget is visible").toBe(0);
  65 | }
  66 | 
  67 | /** No horizontal overflow at the four widths, and the page's h1 once. */
  68 | export async function widths(page: Page) {
  69 |   for (const width of [1024, 1280, 1440, 1920]) {
  70 |     await page.setViewportSize({ width, height: 900 });
  71 |     await page.waitForTimeout(100);
  72 |     expect(await overflowX(page), `overflow at ${width}`).toBeLessThanOrEqual(0);
  73 |     // No table scrolls sideways inside its box either: its columns fit.
  74 |     const wide = await page.evaluate(() => [...document.querySelectorAll("[data-slot=data-table]")].filter((el) => el.scrollWidth > el.clientWidth + 1).map((el) => el.querySelector("table")?.getAttribute("aria-label")));
> 75 |     expect(wide, `tables wider than their box at ${width}`).toEqual([]);
     |                                                             ^ Error: tables wider than their box at 1024
  76 |   }
  77 |   await page.setViewportSize({ width: 1440, height: 900 });
  78 |   await expect(page.locator("h1")).toHaveCount(1);
  79 | }
  80 | 
  81 | /** The page's words: no memory figure anywhere (owner, 2026-11-13). */
  82 | export async function noMemory(page: Page) {
  83 |   const text = (await page.locator("body").innerText()).toLowerCase();
  84 |   for (const word of ["memory", "resident", "rss"]) expect(text, word).not.toContain(word);
  85 | }
  86 | 
```