# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Delete topic >> type the name to confirm; then the topic is gone and the list says so
- Location: tests/topic-writes.spec.ts:239:3

# Error details

```
Error: expect(received).toBe(expected) // Object.is equality

Expected: 404
Received: 200
```

# Page snapshot

```yaml
- generic [ref=e2]:
  - generic [ref=e3]:
    - generic [ref=e6]:
      - link "doze-kafka, Home" [ref=e8] [cursor=pointer]:
        - /url: /
        - generic [ref=e9]: doze-kafka
      - list [ref=e13]:
        - listitem [ref=e14]:
          - link "Home" [ref=e15] [cursor=pointer]:
            - /url: /
        - listitem [ref=e20]:
          - link "Live" [ref=e21] [cursor=pointer]:
            - /url: /live
        - listitem [ref=e25]:
          - link "Topics" [ref=e26] [cursor=pointer]:
            - /url: /topics
          - generic: "19"
        - listitem [ref=e32]:
          - link "Groups" [ref=e33] [cursor=pointer]:
            - /url: /groups
          - generic: "2"
        - listitem [ref=e40]:
          - link "Access" [ref=e41] [cursor=pointer]:
            - /url: /access
        - listitem [ref=e45]:
          - link "Lab" [ref=e46] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=e52]:
          - link "Settings" [ref=e53] [cursor=pointer]:
            - /url: /settings
      - list [ref=e59]:
        - listitem [ref=e60]:
          - link "Classic console" [ref=e61] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=e67]
    - main [ref=e68]:
      - generic [ref=e69]:
        - button "Toggle Sidebar" [ref=e70]
        - button "Jump to a topic or group ⌘K" [ref=e72]:
          - generic [ref=e76]: Jump to a topic or group
          - generic [ref=e77]: ⌘K
        - generic [ref=e78]:
          - button "127.0.0.1:39092" [ref=e79]
          - generic [ref=e84]: Kafka 4 profile
          - link "Open" [ref=e85] [cursor=pointer]:
            - /url: /access
      - main [ref=e87]:
        - generic [ref=e88]:
          - generic [ref=e89]:
            - navigation "Breadcrumb"
            - generic [ref=e90]:
              - generic [ref=e91]: Updated 07:41:20 UTC
              - combobox "Refresh every" [ref=e92]:
                - generic: "Refresh: Off"
              - button "Refresh" [ref=e93]
          - generic [ref=e94]:
            - generic [ref=e95]:
              - heading "Topics" [level=1] [ref=e96]
              - paragraph [ref=e97]: "Where records live. Every topic is a log: open one to read it, produce to it, or see what the broker did to it."
            - button "Create topic" [ref=e99]
        - generic [ref=e100]:
          - generic [ref=e101]:
            - generic [ref=e102]:
              - term [ref=e103]: Topics
              - definition [ref=e105]: "19"
            - generic [ref=e106]:
              - term [ref=e107]: Partitions
              - definition [ref=e109]: "29"
            - generic [ref=e110]:
              - term [ref=e111]: Newest write
              - definition [ref=e113]: 07:41:18
              - generic "UTC" [ref=e114]
            - generic [ref=e115]:
              - term [ref=e116]: Written to
              - definition [ref=e118]: r1b-trimc-firefox-mweo47cg-6
          - search [ref=e119]:
            - textbox "Filter topics by name" [ref=e121]
            - generic [ref=e122]:
              - switch "Show internal" [ref=e123]
              - generic [ref=e124] [cursor=pointer]: Show internal
          - table "Topics" [ref=e126]:
            - rowgroup [ref=e133]:
              - row [ref=e134]:
                - columnheader "Name" [ref=e135]:
                  - button "Name" [ref=e136]
                - columnheader "Partitions" [ref=e139]:
                  - button "Partitions" [ref=e140]
                - columnheader "Cleanup policy" [ref=e143]:
                  - button "Cleanup policy" [ref=e144]
                - columnheader "Size on disk" [ref=e147]:
                  - button "Size on disk" [ref=e148]
                - columnheader "Last write" [ref=e151]:
                  - button "Last write" [ref=e152]
            - rowgroup [ref=e155]:
              - row [ref=e156]:
                - cell [ref=e157]:
                  - link "inventory.levels" [ref=e160] [cursor=pointer]:
                    - /url: /topics/inventory.levels/messages
                - cell "1" [ref=e161]
                - cell "compact" [ref=e162]
                - cell "773 B" [ref=e164]
                - cell "07:38:16" [ref=e165]
              - row [ref=e166]:
                - cell "orders read by fulfilment" [ref=e167]:
                  - generic [ref=e168]:
                    - link "orders" [ref=e170] [cursor=pointer]:
                      - /url: /topics/orders/messages
                    - generic "read by fulfilment" [ref=e171]
                - cell "3" [ref=e172]
                - cell "delete" [ref=e173]
                - cell "2.9 KiB" [ref=e175]
                - cell "07:40:18" [ref=e176]
              - row [ref=e177]:
                - cell [ref=e178]:
                  - link "payments.settled" [ref=e181] [cursor=pointer]:
                    - /url: /topics/payments.settled/messages
                - cell "2" [ref=e182]
                - cell "delete" [ref=e183]
                - cell "2.2 KiB" [ref=e185]
                - cell "07:40:10" [ref=e186]
              - row [ref=e187]:
                - 'cell "r1b-cfg-firefox-mweo3p5d-1 empty: no records yet" [ref=e188]':
                  - generic [ref=e189]:
                    - link "r1b-cfg-firefox-mweo3p5d-1" [ref=e191] [cursor=pointer]:
                      - /url: /topics/r1b-cfg-firefox-mweo3p5d-1/messages
                    - 'generic "empty: no records yet" [ref=e192]'
                - cell "1" [ref=e193]
                - cell "delete" [ref=e194]
                - cell "0 B" [ref=e196]
                - cell "never" [ref=e197]
              - row [ref=e198]:
                - 'cell "r1b-cfge-firefox-mweo3pzf-3 empty: no records yet" [ref=e199]':
                  - generic [ref=e200]:
                    - link "r1b-cfge-firefox-mweo3pzf-3" [ref=e202] [cursor=pointer]:
                      - /url: /topics/r1b-cfge-firefox-mweo3pzf-3/messages
                    - 'generic "empty: no records yet" [ref=e203]'
                - cell "1" [ref=e204]
                - cell "delete" [ref=e205]
                - cell "0 B" [ref=e207]
                - cell "never" [ref=e208]
              - row [ref=e209]:
                - 'cell "r1b-cfgg-firefox-mweo3rk8-5 empty: no records yet" [ref=e210]':
                  - generic [ref=e211]:
                    - link "r1b-cfgg-firefox-mweo3rk8-5" [ref=e213] [cursor=pointer]:
                      - /url: /topics/r1b-cfgg-firefox-mweo3rk8-5/messages
                    - 'generic "empty: no records yet" [ref=e214]'
                - cell "1" [ref=e215]
                - cell "compact" [ref=e216]
                - cell "0 B" [ref=e218]
                - cell "never" [ref=e219]
              - row [ref=e220]:
                - 'cell "r1b-cfgl-firefox-mweo3r4m-4 empty: no records yet" [ref=e221]':
                  - generic [ref=e222]:
                    - link "r1b-cfgl-firefox-mweo3r4m-4" [ref=e224] [cursor=pointer]:
                      - /url: /topics/r1b-cfgl-firefox-mweo3r4m-4/messages
                    - 'generic "empty: no records yet" [ref=e225]'
                - cell "1" [ref=e226]
                - cell "compact,delete" [ref=e227]
                - cell "0 B" [ref=e229]
                - cell "never" [ref=e230]
              - row [ref=e231]:
                - 'cell "r1b-cfgp-firefox-mweo3pq3-2 empty: no records yet" [ref=e232]':
                  - generic [ref=e233]:
                    - link "r1b-cfgp-firefox-mweo3pq3-2" [ref=e235] [cursor=pointer]:
                      - /url: /topics/r1b-cfgp-firefox-mweo3pq3-2/messages
                    - 'generic "empty: no records yet" [ref=e236]'
                - cell "1" [ref=e237]
                - cell "delete" [ref=e238]
                - cell "0 B" [ref=e240]
                - cell "never" [ref=e241]
              - row [ref=e242]:
                - 'cell "r1b-cfgr-firefox-mweo3t1j-7 empty: no records yet" [ref=e243]':
                  - generic [ref=e244]:
                    - link "r1b-cfgr-firefox-mweo3t1j-7" [ref=e246] [cursor=pointer]:
                      - /url: /topics/r1b-cfgr-firefox-mweo3t1j-7/messages
                    - 'generic "empty: no records yet" [ref=e247]'
                - cell "1" [ref=e248]
                - cell "delete" [ref=e249]
                - cell "0 B" [ref=e251]
                - cell "never" [ref=e252]
              - row [ref=e253]:
                - 'cell "r1b-cfgv-firefox-mweo3s8k-6 empty: no records yet" [ref=e254]':
                  - generic [ref=e255]:
                    - link "r1b-cfgv-firefox-mweo3s8k-6" [ref=e257] [cursor=pointer]:
                      - /url: /topics/r1b-cfgv-firefox-mweo3s8k-6/messages
                    - 'generic "empty: no records yet" [ref=e258]'
                - cell "1" [ref=e259]
                - cell "delete" [ref=e260]
                - cell "0 B" [ref=e262]
                - cell "never" [ref=e263]
              - row [ref=e264]:
                - cell "r1b-compacted-firefox-mweo435t-2 retention 7 d" [ref=e265]:
                  - generic [ref=e266]:
                    - link "r1b-compacted-firefox-mweo435t-2" [ref=e268] [cursor=pointer]:
                      - /url: /topics/r1b-compacted-firefox-mweo435t-2/messages
                    - generic "retention 7 d" [ref=e269]
                - cell "1" [ref=e270]
                - cell "compact" [ref=e271]
                - cell "70 B" [ref=e273]
                - cell "07:41:13" [ref=e274]
              - row [ref=e275]:
                - 'cell "r1b-grow-firefox-mweo44l6-4 empty: no records yet" [ref=e276]':
                  - generic [ref=e277]:
                    - link "r1b-grow-firefox-mweo44l6-4" [ref=e279] [cursor=pointer]:
                      - /url: /topics/r1b-grow-firefox-mweo44l6-4/messages
                    - 'generic "empty: no records yet" [ref=e280]'
                - cell "3" [ref=e281]
                - cell "delete" [ref=e282]
                - cell "0 B" [ref=e284]
                - cell "never" [ref=e285]
              - row [ref=e286]:
                - 'cell "r1b-keyed-firefox-mweo40ap-2 empty: no records yet" [ref=e287]':
                  - generic [ref=e288]:
                    - link "r1b-keyed-firefox-mweo40ap-2" [ref=e290] [cursor=pointer]:
                      - /url: /topics/r1b-keyed-firefox-mweo40ap-2/messages
                    - 'generic "empty: no records yet" [ref=e291]'
                - cell "3" [ref=e292]
                - cell "delete" [ref=e293]
                - cell "0 B" [ref=e295]
                - cell "never" [ref=e296]
              - row [ref=e297]:
                - cell "r1b-keyless-firefox-mweo41xq-1 retention 7 d" [ref=e298]:
                  - generic [ref=e299]:
                    - link "r1b-keyless-firefox-mweo41xq-1" [ref=e301] [cursor=pointer]:
                      - /url: /topics/r1b-keyless-firefox-mweo41xq-1/messages
                    - generic "retention 7 d" [ref=e302]
                - cell "1" [ref=e303]
                - cell "delete" [ref=e304]
                - cell "79 B" [ref=e306]
                - cell "07:41:12" [ref=e307]
              - row [ref=e308]:
                - 'cell "r1b-large-firefox-mweo43se-3 empty: no records yet" [ref=e309]':
                  - generic [ref=e310]:
                    - link "r1b-large-firefox-mweo43se-3" [ref=e312] [cursor=pointer]:
                      - /url: /topics/r1b-large-firefox-mweo43se-3/messages
                    - 'generic "empty: no records yet" [ref=e313]'
                - cell "1" [ref=e314]
                - cell "delete" [ref=e315]
                - cell "0 B" [ref=e317]
                - cell "never" [ref=e318]
              - row [ref=e319]:
                - 'cell "r1b-race-firefox-mweo3zfc-1 empty: no records yet" [ref=e320]':
                  - generic [ref=e321]:
                    - link "r1b-race-firefox-mweo3zfc-1" [ref=e323] [cursor=pointer]:
                      - /url: /topics/r1b-race-firefox-mweo3zfc-1/messages
                    - 'generic "empty: no records yet" [ref=e324]'
                - cell "1" [ref=e325]
                - cell "delete" [ref=e326]
                - cell "0 B" [ref=e328]
                - cell "never" [ref=e329]
              - row [ref=e330]:
                - cell "r1b-trim-firefox-mweo46ca-5 retention 7 d" [ref=e331]:
                  - generic [ref=e332]:
                    - link "r1b-trim-firefox-mweo46ca-5" [ref=e334] [cursor=pointer]:
                      - /url: /topics/r1b-trim-firefox-mweo46ca-5/messages
                    - generic "retention 7 d" [ref=e335]
                - cell "2" [ref=e336]
                - cell "delete" [ref=e337]
                - cell "360 B" [ref=e339]
                - cell "07:41:17" [ref=e340]
              - row [ref=e341]:
                - cell "r1b-trimc-firefox-mweo47cg-6 retention 7 d" [ref=e342]:
                  - generic [ref=e343]:
                    - link "r1b-trimc-firefox-mweo47cg-6" [ref=e345] [cursor=pointer]:
                      - /url: /topics/r1b-trimc-firefox-mweo47cg-6/messages
                    - generic "retention 7 d" [ref=e346]
                - cell "1" [ref=e347]
                - cell "compact" [ref=e348]
                - cell "140 B" [ref=e350]
                - cell "07:41:18" [ref=e351]
              - row [ref=e352]:
                - cell "r2-orders read by r2-busy-firefox" [ref=e353]:
                  - generic [ref=e354]:
                    - link "r2-orders" [ref=e356] [cursor=pointer]:
                      - /url: /topics/r2-orders/messages
                    - generic "read by r2-busy-firefox" [ref=e357]
                - cell "3" [ref=e358]
                - cell "delete" [ref=e359]
                - cell "703 B" [ref=e361]
                - cell "07:38:38" [ref=e362]
          - paragraph [ref=e363]: 19 of 19 topics
          - generic [ref=e369]:
            - paragraph [ref=e370]:
              - text: One internal topic is hidden. Turn on Show internal and
              - code [ref=e371]: __consumer_offsets
              - text: joins the list.
            - paragraph [ref=e372]:
              - text: Why it is read only.
              - code [ref=e373]: __consumer_offsets
              - text: "is Kafka's own name for the place the group coordinator keeps each group's committed offsets, spread over 50 partitions. This broker keeps commits in its own store and answers Kafka's offset calls from it, so there is no log here to read, produce to, delete from, or change. The name is shown because Kafka clients and tools know it; the 50 partitions decide which coordinator slot a group lands in. Look at a group's commits on its Lag tab instead:"
              - link "open Groups" [ref=e374] [cursor=pointer]:
                - /url: /groups
              - text: .
      - generic [ref=e375]:
        - heading "Jump to a topic or group" [level=2] [ref=e376]
        - paragraph [ref=e377]: Type a name, then Enter
  - region "Notifications alt+T":
    - list:
      - listitem [ref=e378]:
        - generic [ref=e383]:
          - generic [ref=e384]: Deleted r1b-gone-firefox-mweo486a-7.
          - generic [ref=e385]: "1 record and 2 partitions are gone from disk. A topic named r1b-gone-firefox-mweo486a-7 made later is a new topic: it starts at offset 0."
```

# Test source

```ts
  154 |     await expect(d.getByTestId("refusal-code")).toHaveText("INVALID_RECORD (87)");
  155 |     const api = await call(request, baseURL!, `/api/v1/topics/${name}/produce`, { key: "", value: "v", partition: "", acks: "-1" });
  156 |     await expect(d.getByTestId("refusal-message")).toHaveText(api.body.message);
  157 |     await expect(d.getByTestId("field-error")).toHaveText("A compacted topic needs a key on every record.");
  158 |     // With a key it goes.
  159 |     await d.getByTestId("key").fill("k");
  160 |     await d.getByTestId("send").click();
  161 |     await expect(d.getByTestId("produced")).toBeVisible();
  162 |   });
  163 | 
  164 |   test("a record over max.message.bytes: refused MESSAGE_TOO_LARGE", async ({ page, request, baseURL }, info) => {
  165 |     const name = fresh(info, "large");
  166 |     await topic(request, baseURL!, name, 1);
  167 |     await setConfig(request, baseURL!, name, "max.message.bytes", "200");
  168 |     await page.goto(dialogUrl(name, "produce"));
  169 |     const d = page.getByTestId("dialog-produce");
  170 |     await d.getByTestId("key").fill("k");
  171 |     await d.getByTestId("value").fill("x".repeat(600));
  172 |     await d.getByTestId("send").click();
  173 |     await expect(d.getByTestId("refusal-code")).toHaveText("MESSAGE_TOO_LARGE (10)");
  174 |     await expect(d.getByTestId("the-answer")).toBeVisible();
  175 |   });
  176 | });
  177 | 
  178 | test.describe("Add partitions", () => {
  179 |   test("the new total, what it adds, then the broker's count; fewer is refused in Kafka's words", async ({ page, request, baseURL }, info) => {
  180 |     const name = fresh(info, "grow");
  181 |     await topic(request, baseURL!, name, 1);
  182 |     await page.goto(dialogUrl(name, "add-partitions"));
  183 |     const d = page.getByTestId("dialog-add-partitions");
  184 |     await expect(d.getByTestId("count")).toHaveValue("2");
  185 |     await expect(d.getByTestId("adds")).toHaveText(`adds 1: ${name}-1`);
  186 |     await d.getByTestId("count").fill("3");
  187 |     await expect(d.getByTestId("add-submit")).toHaveText("Add 2 partitions");
  188 |     await expect(d.getByTestId("the-call")).toContainText("count: 3");
  189 |     await d.getByTestId("add-submit").click();
  190 |     await expect(d.getByTestId("added-to")).toHaveText("3");
  191 |     expect((await get(request, `/api/v1/topics/${name}/add-partitions`)).body.view.have).toBe(3);
  192 |     await d.getByRole("button", { name: "Done" }).click();
  193 |     await expect(page).not.toHaveURL(/dialog=/);
  194 |     // Fewer than now: Kafka's refusal, in the dialog.
  195 |     await page.goto(dialogUrl(name, "add-partitions"));
  196 |     await d.getByTestId("count").fill("2");
  197 |     await d.getByTestId("add-submit").click();
  198 |     await expect(d.getByTestId("refusal-code")).toHaveText("INVALID_PARTITIONS (37)");
  199 |     await expect(d.getByTestId("the-answer")).toBeVisible();
  200 |   });
  201 | });
  202 | 
  203 | test.describe("Delete records", () => {
  204 |   test("the preview says what goes and the button counts it; the log starts there after", async ({ page, request, baseURL }, info) => {
  205 |     const name = fresh(info, "trim");
  206 |     await topic(request, baseURL!, name, 2);
  207 |     for (let i = 0; i < 5; i++) await produce(request, baseURL!, name, `k${i}`, `v${i}`, "1");
  208 |     await page.goto(dialogUrl(name, "delete-records", "&partition=0"));
  209 |     const d = page.getByTestId("dialog-delete-records");
  210 |     // A deep link's partition is shown in the trigger before anything is chosen.
  211 |     await expect(d.getByTestId("records-partition")).toHaveText("0");
  212 |     await choose(page, d.getByTestId("records-partition"), "1");
  213 |     await expect(d.getByTestId("records-partition")).toHaveText("1");
  214 |     await expect(d.getByTestId("records-preview")).toContainText("This removes 0 records");
  215 |     await d.getByTestId("offset").fill("3");
  216 |     await expect(d.getByTestId("records-preview")).toContainText("This removes 3 records");
  217 |     await expect(d.getByTestId("records-submit")).toHaveText("Delete 3 records");
  218 |     await d.getByTestId("records-submit").click();
  219 |     await expect(d.getByTestId("low-watermark")).toHaveText("3");
  220 |     const after = await get(request, `/api/v1/topics/${name}/delete-records?partition=1`);
  221 |     expect(after.body.view.log_start).toBe(3);
  222 |   });
  223 | 
  224 |   test("on a topic that only compacts: refused POLICY_VIOLATION", async ({ page, request, baseURL }, info) => {
  225 |     const name = fresh(info, "trimc");
  226 |     await topic(request, baseURL!, name, 1, "compact");
  227 |     await produce(request, baseURL!, name, "a", "1");
  228 |     await produce(request, baseURL!, name, "b", "2");
  229 |     await page.goto(dialogUrl(name, "delete-records", "&partition=0"));
  230 |     const d = page.getByTestId("dialog-delete-records");
  231 |     await d.getByTestId("offset").fill("1");
  232 |     await d.getByTestId("records-submit").click();
  233 |     await expect(d.getByTestId("refusal-code")).toHaveText("POLICY_VIOLATION (44)");
  234 |     await expect(d.getByTestId("records-refused")).toContainText("log start is still 0");
  235 |   });
  236 | });
  237 | 
  238 | test.describe("Delete topic", () => {
  239 |   test("type the name to confirm; then the topic is gone and the list says so", async ({ page, request, baseURL }, info) => {
  240 |     const name = fresh(info, "gone");
  241 |     await topic(request, baseURL!, name, 2);
  242 |     await produce(request, baseURL!, name, "k", "v");
  243 |     await page.goto(`/topics/${name}`);
  244 |     await page.getByTestId("act-delete").click();
  245 |     const d = page.getByTestId("dialog-delete");
  246 |     await expect(d.getByTestId("delete-what")).toContainText("1 record");
  247 |     await expect(d.getByTestId("delete-submit")).toBeDisabled();
  248 |     await d.getByTestId("confirm").fill(name.slice(0, -1));
  249 |     await expect(d.getByTestId("delete-submit")).toBeDisabled();
  250 |     await d.getByTestId("confirm").fill(name);
  251 |     await d.getByTestId("delete-submit").click();
  252 |     await expect(page).toHaveURL(/\/topics$/);
  253 |     await expect(page.locator("[data-sonner-toast]").filter({ hasText: `Deleted ${name}.` })).toBeVisible();
> 254 |     expect((await get(request, `/api/v1/topics/${name}/delete`)).status).toBe(404);
      |                                                                          ^ Error: expect(received).toBe(expected) // Object.is equality
  255 |   });
  256 | 
  257 |   test("a topic a group reads says it is in use", async ({ page }) => {
  258 |     await page.goto(dialogUrl("orders", "delete"));
  259 |     const d = page.getByTestId("dialog-delete");
  260 |     await expect(d.getByTestId("in-use")).toContainText("fulfilment");
  261 |     await page.keyboard.press("Escape");
  262 |     await expect(d).toBeHidden();
  263 |   });
  264 | });
  265 | 
```