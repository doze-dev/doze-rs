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
+   "Intervals",
+   "Flags",
+ ]
```

# Page snapshot

```yaml
- generic [ref=e2]:
  - generic [ref=e3]:
    - generic [ref=e6]:
      - link "doze Kafka, Home" [ref=e8] [cursor=pointer]:
        - /url: /
        - generic [ref=e9]:
          - generic [ref=e10]: doze Kafka
          - text: doze
          - generic [aria-hidden] [ref=e11]: Kafka
      - list [ref=e15]:
        - listitem [ref=e16]:
          - link "Home" [ref=e17] [cursor=pointer]:
            - /url: /
        - listitem [ref=e22]:
          - link "Live" [ref=e23] [cursor=pointer]:
            - /url: /live
        - listitem [ref=e27]:
          - link "Topics" [ref=e28] [cursor=pointer]:
            - /url: /topics
          - generic: "4"
        - listitem [ref=e34]:
          - link "Groups" [ref=e35] [cursor=pointer]:
            - /url: /groups
          - generic: "3"
        - listitem [ref=e42]:
          - link "Access" [ref=e43] [cursor=pointer]:
            - /url: /access
        - listitem [ref=e47]:
          - link "Lab" [ref=e48] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=e54]:
          - link "Settings" [ref=e55] [cursor=pointer]:
            - /url: /settings
      - list [ref=e61]:
        - listitem [ref=e62]:
          - link "Classic console" [ref=e63] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=e69]
    - main [ref=e70]:
      - generic [ref=e71]:
        - button "Toggle Sidebar" [ref=e72]
        - button "Jump to a topic or group ⌘K" [ref=e74]:
          - generic [ref=e78]: Jump to a topic or group
          - generic [ref=e79]: ⌘K
        - generic [ref=e80]:
          - button "127.0.0.1:39092" [ref=e81]
          - link "Open" [ref=e86] [cursor=pointer]:
            - /url: /access
          - radiogroup "Theme" [ref=e88]:
            - radio "System" [checked] [ref=e89]
            - radio "Light" [ref=e94]
            - radio "Dark" [ref=e105]
      - main [ref=e108]:
        - generic [ref=e111]:
          - heading "Settings" [level=1] [ref=e112]
          - paragraph [ref=e113]: This broker's own configuration, explained. Five settings can change while it runs; everything else was fixed when it started and is shown read only.
        - generic [ref=e114]:
          - generic [ref=e115]:
            - term [ref=e116]: Set at run time
            - definition [ref=e118]: 0 of 5
            - generic "all at their start values" [ref=e119]
          - generic [ref=e120]:
            - term [ref=e121]: Running since
            - definition [ref=e123]: Nov 13 08:56
            - generic "UTC" [ref=e124]
          - generic [ref=e125]:
            - term [ref=e126]: Version profile
            - definition [ref=e128]: Kafka 4
            - generic "--kafka-version" [ref=e129]
        - navigation "Sections" [ref=e130]:
          - link "Broker configs" [ref=e131] [cursor=pointer]:
            - /url: "#configs"
          - link "Version profile" [ref=e132] [cursor=pointer]:
            - /url: "#profile"
          - link "Intervals" [ref=e133] [cursor=pointer]:
            - /url: "#intervals"
          - link "Weight" [ref=e134] [cursor=pointer]:
            - /url: "#weight"
          - link "Data" [ref=e135] [cursor=pointer]:
            - /url: "#data"
          - link "Flags" [ref=e136] [cursor=pointer]:
            - /url: "#flags"
        - region "Broker configs" [ref=e137]:
          - generic [ref=e139]:
            - heading "Broker configs" [level=2] [ref=e140]
            - paragraph [ref=e141]: "Source says where the value in force comes from: a dynamic setting first, then the flag it started with, then Kafka's own default. A topic that sets its own value keeps it."
          - table "Dynamic broker configs" [ref=e143]:
            - rowgroup [ref=e149]:
              - row [ref=e150]:
                - columnheader "Config" [ref=e151]:
                  - button "Config" [ref=e152]
                - columnheader "Value" [ref=e155]:
                  - button "Value" [ref=e156]
                - columnheader "Source" [ref=e159]:
                  - button "Source" [ref=e160]
                - columnheader "Edit" [ref=e163]
            - rowgroup [ref=e165]:
              - row [ref=e166]:
                - cell "log.segment.bytes" [ref=e167]
                - cell "134217728128 MiB" [ref=e168]:
                  - generic "134217728 (128 MiB)" [ref=e169]: 134217728128 MiB
                - cell [ref=e170]:
                  - 'generic "Static broker: flag --segment-bytes; Kafka''s default is 1 GiB" [ref=e171]': Static broker
                - cell [ref=e174]:
                  - button "Edit log.segment.bytes" [ref=e175]: Edit
              - row [ref=e176]:
                - cell "log.index.interval.bytes" [ref=e177]
                - cell "40964 KiB" [ref=e178]:
                  - generic "4096 (4 KiB)" [ref=e179]: 40964 KiB
                - cell [ref=e180]:
                  - 'generic "Default: Kafka''s default" [ref=e181]': Default
                - cell [ref=e184]:
                  - button "Edit log.index.interval.bytes" [ref=e185]: Edit
              - row [ref=e186]:
                - cell "message.max.bytes" [ref=e187]
                - cell "10485881 MiB + 12 B" [ref=e188]:
                  - generic "1048588 (1 MiB + 12 B)" [ref=e189]: 10485881 MiB + 12 B
                - cell [ref=e190]:
                  - 'generic "Default: Kafka''s default" [ref=e191]': Default
                - cell [ref=e194]:
                  - button "Edit message.max.bytes" [ref=e195]: Edit
              - row [ref=e196]:
                - cell "log.retention.bytes" [ref=e197]
                - cell "-1no limit" [ref=e198]:
                  - generic "-1 (no limit)" [ref=e199]: "-1no limit"
                - cell [ref=e200]:
                  - 'generic "Default: Kafka''s default" [ref=e201]': Default
                - cell [ref=e204]:
                  - button "Edit log.retention.bytes" [ref=e205]: Edit
              - row [ref=e206]:
                - cell "log.retention.ms" [ref=e207]
                - cell "-1no limit" [ref=e208]:
                  - generic "-1 (no limit)" [ref=e209]: "-1no limit"
                - cell [ref=e210]:
                  - 'generic "Static broker: flag --retention-ms; Kafka''s default is 7 d" [ref=e211]': Static broker
                - cell [ref=e214]:
                  - button "Edit log.retention.ms" [ref=e215]: Edit
          - paragraph [ref=e216]: "A change is kept on disk and is still there after a restart. It applies to all brokers or to this one (node 1). These are the only five: any other config is refused, in Kafka's words."
        - region "Version profile" [ref=e217]:
          - generic [ref=e219]:
            - heading "Version profile" [level=2] [ref=e220]
            - paragraph [ref=e221]: The Kafka the broker stands for, and the APIs a client is offered, each with the versions a real Kafka 4 broker offers. It is what ApiVersions answers. A client picks, for each API, the highest version both sides have.
          - generic [ref=e222]:
            - generic [ref=e223]:
              - term [ref=e224]: Profile
              - definition [ref=e226]: Kafka 4
              - generic "--kafka-version" [ref=e227]
            - generic [ref=e228]:
              - term [ref=e229]: APIs offered
              - definition [ref=e231]: "41"
              - generic "SASL off" [ref=e232]
            - generic [ref=e233]:
              - term [ref=e234]: Not offered
              - definition [ref=e236]: "2"
              - generic "of this Kafka's APIs" [ref=e237]
          - list [ref=e238]:
            - listitem [ref=e239]:
              - code [ref=e240]: lab-once-firefox
              - generic [ref=e241]: speaks
              - code [ref=e242]: Fetch v11
              - generic [ref=e243]: inside the range
            - listitem [ref=e245]:
              - code [ref=e246]: e2e-tail
              - generic [ref=e247]: speaks
              - code [ref=e248]: Produce v7
              - generic [ref=e249]: inside the range
          - table "APIs offered" [ref=e252]:
            - rowgroup [ref=e257]:
              - row [ref=e258]:
                - columnheader "API" [ref=e259]:
                  - button "API" [ref=e260]
                - columnheader "Versions" [ref=e263]:
                  - button "Versions" [ref=e264]
                - columnheader "Family" [ref=e267]:
                  - button "Family" [ref=e268]
            - rowgroup [ref=e271]:
              - row [ref=e272]:
                - cell "Produce" [ref=e273]
                - cell "v3 to v9" [ref=e274]
                - cell "Produce and fetch" [ref=e275]
              - row [ref=e276]:
                - cell "Fetch" [ref=e277]
                - cell "v4 to v13" [ref=e278]
                - cell "Produce and fetch" [ref=e279]
              - row [ref=e280]:
                - cell "ListOffsets" [ref=e281]
                - cell "v1 to v7" [ref=e282]
                - cell "Produce and fetch" [ref=e283]
              - row [ref=e284]:
                - cell "DeleteRecords" [ref=e285]
                - cell "v0 to v2" [ref=e286]
                - cell "Produce and fetch" [ref=e287]
              - row [ref=e288]:
                - cell "ApiVersions" [ref=e289]
                - cell "v0 to v4" [ref=e290]
                - cell "Metadata and bootstrapping" [ref=e291]
              - row [ref=e292]:
                - cell "Metadata" [ref=e293]
                - cell "v0 to v12" [ref=e294]
                - cell "Metadata and bootstrapping" [ref=e295]
              - row [ref=e296]:
                - cell "FindCoordinator" [ref=e297]
                - cell "v0 to v4" [ref=e298]
                - cell "Metadata and bootstrapping" [ref=e299]
              - row [ref=e300]:
                - cell "DescribeCluster" [ref=e301]
                - cell "v0 to v2" [ref=e302]
                - cell "Metadata and bootstrapping" [ref=e303]
          - button "Show all 41 APIs" [ref=e304]
          - paragraph [ref=e306]: "Not offered: SaslHandshake, SaslAuthenticate (no handler yet)."
        - region "Intervals" [ref=e307]:
          - generic [ref=e309]:
            - heading "Intervals" [level=2] [ref=e310]
            - paragraph [ref=e311]: "How often the broker looks at things on its own. Read only: set when the broker starts. To change an interval, stop the broker and start it with the flag."
          - generic [ref=e312]:
            - generic [ref=e313]:
              - term [ref=e314]: Retention check
              - definition [ref=e316]: 5 m
              - generic "every" [ref=e317]
            - generic [ref=e318]:
              - term [ref=e319]: Cleaner
              - definition [ref=e321]: 15 s
              - generic "looks every" [ref=e322]
            - generic [ref=e323]:
              - term [ref=e324]: Transactions
              - definition [ref=e326]: 500 ms
              - generic "checked every; no flag sets it" [ref=e327]
            - generic [ref=e328]:
              - term [ref=e329]: Last cleaner round
              - definition [ref=e331]: none yet
              - generic "since the broker began" [ref=e332]
          - table "Intervals" [ref=e334]:
            - rowgroup [ref=e340]:
              - row [ref=e341]:
                - columnheader "Interval" [ref=e342]:
                  - button "Interval" [ref=e343]
                - columnheader "Value" [ref=e346]:
                  - button "Value" [ref=e347]
                - columnheader "Flag" [ref=e350]:
                  - button "Flag" [ref=e351]
                - columnheader "What it comes to" [ref=e354]:
                  - button "What it comes to" [ref=e355]
            - rowgroup [ref=e358]:
              - row [ref=e359]:
                - cell "Retention check" [ref=e360]
                - cell "5m" [ref=e361]
                - cell "--log-retention-check-interval" [ref=e362]
                - cell "Must be above 0" [ref=e363]
              - row [ref=e364]:
                - cell "Cleaner look" [ref=e365]
                - cell "15s" [ref=e366]
                - cell "--log-cleaner-interval" [ref=e367]
                - cell "Must be above 0" [ref=e368]
              - row [ref=e369]:
                - cell "Flush after a write" [ref=e370]
                - cell "500ms" [ref=e371]
                - cell "--fsync-interval" [ref=e372]
                - cell "acks=all flushes before it answers" [ref=e373]
              - row [ref=e374]:
                - cell "Offsets retention" [ref=e375]
                - cell "168h" [ref=e376]
                - cell "--offsets-retention" [ref=e377]
                - cell "kept for 7 d; fulfilment's expire in 6 d 23 h" [ref=e378]
              - row [ref=e379]:
                - cell "First rebalance wait" [ref=e380]
                - cell "200ms" [ref=e381]
                - cell "--group-initial-rebalance-delay" [ref=e382]
                - cell "0 does not wait" [ref=e383]
              - row [ref=e384]:
                - cell "Consumer session" [ref=e385]
                - cell "45s" [ref=e386]
                - cell "--consumer-session-timeout" [ref=e387]
                - cell "Must be above 0" [ref=e388]
        - region "Weight" [ref=e389]:
          - generic [ref=e391]:
            - heading "Weight" [level=2] [ref=e392]
            - paragraph [ref=e393]: "What this instance costs the machine on disk and in open partitions. Weight is a feature: the binary's size and its packages are measured in the tests and may only grow with a written reason."
          - generic [ref=e394]:
            - generic [ref=e395]:
              - term [ref=e396]: Binary
              - definition [ref=e398]: 6.6 MB
              - generic "the broker program, console included" [ref=e399]
            - generic [ref=e400]:
              - term [ref=e401]: Open partitions
              - definition [ref=e403]: 9 of 512
              - generic "--max-open-partitions 512" [ref=e404]
            - generic [ref=e405]:
              - term [ref=e406]: Partitions
              - definition [ref=e408]: "9"
              - generic "in 4 topics" [ref=e409]
        - region "Data" [ref=e410]:
          - generic [ref=e412]:
            - heading "Data" [level=2] [ref=e413]
            - paragraph [ref=e414]: Everything this broker keeps lives in one directory. Stop it cleanly and the directory can be copied, moved or deleted as a whole.
          - generic [ref=e415]:
            - generic [ref=e416]:
              - generic [ref=e417]: The data directory
              - generic [ref=e419]:
                - generic [ref=e420]:
                  - code [ref=e421]: /private/var/folders/0m/v8tqg5p94913qp06y45gcd580000gn/T/doze-e2e-blerR1
                  - button "Copy path" [ref=e422]
                - generic [ref=e424]:
                  - generic [ref=e425]:
                    - term [ref=e426]: Set by
                    - definition [ref=e427]:
                      - code [ref=e429]: "--data-dir"
                      - generic [ref=e430]: given at start
                  - generic [ref=e431]:
                    - term [ref=e432]: Size
                    - definition [ref=e433]:
                      - generic [ref=e434]: 139 KiB
                      - generic [ref=e435]: 20 files
                  - generic [ref=e436]:
                    - term [ref=e437]: Partitions on disk
                    - definition [ref=e438]:
                      - generic [ref=e439]: "9"
                      - generic [ref=e440]: in 4 topics
                  - generic [ref=e441]:
                    - term [ref=e442]: Last stop
                    - definition [ref=e443]:
                      - generic [ref=e444]: None yet
                      - generic [ref=e445]: this is a new data directory
            - generic [ref=e446]:
              - generic [ref=e447]: What a restart does
              - generic [ref=e449]:
                - paragraph [ref=e450]: "SIGTERM or Ctrl-C is a clean stop: every open partition is flushed and closed and the stop is recorded, while a kill is not, and the next start distrusts every partition and the first flush covers what it reads back (nothing acknowledged is lost)."
                - paragraph [ref=e451]: Your data is where it was, including topics, records, configs, committed offsets, producer ids and open transactions, while connections, group members, the wire's rows and Lab rules start again from nothing. Stored ACLs and the users made over the API stay; users from flags are re-applied.
        - region "Flags" [ref=e452]:
          - generic [ref=e453]:
            - generic [ref=e454]:
              - heading "Flags" [level=2] [ref=e455]
              - paragraph [ref=e456]: "What the broker was started with, each with its default. Read only: set at start."
            - radiogroup "Which flags" [ref=e458]:
              - radio "Differ from default · 3" [checked] [ref=e459]
              - radio "All · 39" [ref=e460]
          - table "Flags" [ref=e462]:
            - rowgroup [ref=e468]:
              - row [ref=e469]:
                - columnheader "Flag" [ref=e470]:
                  - button "Flag" [ref=e471]
                - columnheader "Value" [ref=e474]:
                  - button "Value" [ref=e475]
                - columnheader "Default" [ref=e478]:
                  - button "Default" [ref=e479]
                - columnheader "What it does" [ref=e482]:
                  - button "What it does" [ref=e483]
            - rowgroup [ref=e486]:
              - row [ref=e487]:
                - cell "--listen" [ref=e488]
                - cell "127.0.0.1:39092" [ref=e489]
                - cell "127.0.0.1:9092" [ref=e490]
                - cell "set Serve on this host:port" [ref=e491]:
                  - generic "Serve on this host:port" [ref=e492]:
                    - generic [ref=e493]: set
                    - text: Serve on this host:port
              - row [ref=e494]:
                - cell "--data-dir" [ref=e495]
                - cell "/var/folders/0m/v8tqg5p94913qp06y45gcd580000gn/T/doze-e2e-blerR1" [ref=e496]
                - cell "./data" [ref=e497]
                - cell "set Directory for the metadata database and the partition logs" [ref=e498]:
                  - generic "Directory for the metadata database and the partition logs" [ref=e499]:
                    - generic [ref=e500]: set
                    - text: Directory for the metadata database and the partition logs
              - row [ref=e501]:
                - cell "--console-addr" [ref=e502]
                - cell "127.0.0.1:39094" [ref=e503]
                - cell "127.0.0.1:9093" [ref=e504]
                - 'cell "set Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)" [ref=e505]':
                  - 'generic "Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)" [ref=e506]':
                    - generic [ref=e507]: set
                    - text: "Where the console listens (default 127.0.0.1:9093: the Kafka port plus one, on this machine only)"
          - paragraph [ref=e508]:
            - generic [ref=e513]: "A flag that carries a secret says how many were given and never what. A broker that would be open while looking closed refuses to start (exit code 2): with SASL and nobody who could log in, with a certificate it cannot read, or with the console beyond loopback while SASL or TLS is on."
      - generic [ref=e514]:
        - heading "Jump to a topic or group" [level=2] [ref=e515]
        - paragraph [ref=e516]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  1  | // Helpers of the Settings, Access and Lab specs (slice R4): the broker's Kafka address, a client with traffic, the API with the
  2  | // console's proof, and the checks every page of the slice must pass.
  3  | import type { APIRequestContext, Page } from "@playwright/test";
  4  | import { Kafka, logLevel, type Producer } from "kafkajs";
  5  | import { cutHeaders, expect, overflowX } from "./fixtures";
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
  76 |     if (width >= 1280) expect(await cutHeaders(page), `headers cut at ${width}`).toEqual([]);
  77 |   }
  78 |   await page.setViewportSize({ width: 1440, height: 900 });
  79 |   await expect(page.locator("h1")).toHaveCount(1);
  80 | }
  81 | 
  82 | /** The page's words: no memory figure anywhere (owner, 2026-11-13). */
  83 | export async function noMemory(page: Page) {
  84 |   const text = (await page.locator("body").innerText()).toLowerCase();
  85 |   for (const word of ["memory", "resident", "rss"]) expect(text, word).not.toContain(word);
  86 | }
  87 | 
```