# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Produce >> a keyed record with a header to a chosen partition: the offset the log chose, then another
- Location: tests/topic-writes.spec.ts:89:3

# Error details

```
Error: goto: NS_BINDING_ABORTED
Call log:
  - navigating to "http://127.0.0.1:39094/topics/r1b-keyed-firefox-mweo40ap-2?dialog=produce", waiting until "load"

```

# Page snapshot

```yaml
- generic [ref=f1e2]:
  - generic [ref=f1e3]:
    - generic [ref=f1e6]:
      - link "doze-kafka, Home" [ref=f1e8] [cursor=pointer]:
        - /url: /
        - generic [ref=f1e9]: doze-kafka
      - list [ref=f1e13]:
        - listitem [ref=f1e14]:
          - link "Home" [ref=f1e15] [cursor=pointer]:
            - /url: /
        - listitem [ref=f1e20]:
          - link "Live" [ref=f1e21] [cursor=pointer]:
            - /url: /live
        - listitem [ref=f1e25]:
          - link "Topics" [ref=f1e26] [cursor=pointer]:
            - /url: /topics
          - generic: "13"
        - listitem [ref=f1e32]:
          - link "Groups" [ref=f1e33] [cursor=pointer]:
            - /url: /groups
          - generic: "2"
        - listitem [ref=f1e40]:
          - link "Access" [ref=f1e41] [cursor=pointer]:
            - /url: /access
        - listitem [ref=f1e45]:
          - link "Lab" [ref=f1e46] [cursor=pointer]:
            - /url: /lab
        - listitem [ref=f1e52]:
          - link "Settings" [ref=f1e53] [cursor=pointer]:
            - /url: /settings
      - list [ref=f1e59]:
        - listitem [ref=f1e60]:
          - link "Classic console" [ref=f1e61] [cursor=pointer]:
            - /url: /classic/
      - button "Toggle Sidebar" [ref=f1e67]
    - main [ref=f1e68]:
      - generic [ref=f1e69]:
        - button "Toggle Sidebar" [ref=f1e70]
        - button "Jump to a topic or group ⌘K" [ref=f1e72]:
          - generic [ref=f1e76]: Jump to a topic or group
          - generic [ref=f1e77]: ⌘K
        - generic [ref=f1e78]:
          - button "127.0.0.1:39092" [ref=f1e79]
          - generic [ref=f1e84]: Kafka 4 profile
          - link "Open" [ref=f1e85] [cursor=pointer]:
            - /url: /access
      - main [ref=f1e87]:
        - generic [ref=f1e88]:
          - generic [ref=f1e89]:
            - generic [ref=f1e90]:
              - navigation "Breadcrumb" [ref=f1e91]:
                - list [ref=f1e92]:
                  - listitem [ref=f1e93]:
                    - link "Topics" [ref=f1e94] [cursor=pointer]:
                      - /url: /topics
              - generic [ref=f1e95]:
                - generic [ref=f1e96]: Updated 07:41:09 UTC
                - combobox "Refresh every" [ref=f1e97]:
                  - generic: "Refresh: Off"
                - button "Refresh" [ref=f1e98]
            - generic [ref=f1e99]:
              - generic [ref=f1e100]:
                - heading "r1b-keyed-firefox-mweo40ap-2" [level=1] [ref=f1e101]
                - generic [ref=f1e102]: delete
              - generic [ref=f1e104]:
                - button "Produce" [ref=f1e105]
                - button "Delete topic" [ref=f1e106]
          - generic [ref=f1e107]:
            - generic [ref=f1e108]:
              - term [ref=f1e109]: Partitions
              - definition [ref=f1e111]: "3"
              - generic "all online" [ref=f1e112]
            - generic [ref=f1e113]:
              - term [ref=f1e114]: Retention
              - definition [ref=f1e116]: 7 d
              - generic "delete" [ref=f1e117]
            - generic [ref=f1e118]:
              - term [ref=f1e119]: Records
              - definition [ref=f1e121]: "0"
            - generic [ref=f1e122]:
              - term [ref=f1e123]: Size on disk
              - definition [ref=f1e125]: 0 B
            - generic [ref=f1e126]:
              - term [ref=f1e127]: Last write
              - definition [ref=f1e129]: never
          - generic [ref=f1e130]: No open transactions.
          - navigation "Topic" [ref=f1e135]:
            - list [ref=f1e136]:
              - listitem [ref=f1e137]:
                - link "Messages" [ref=f1e138] [cursor=pointer]:
                  - /url: /topics/r1b-keyed-firefox-mweo40ap-2/messages
              - listitem [ref=f1e139]:
                - link "Log" [ref=f1e140] [cursor=pointer]:
                  - /url: /topics/r1b-keyed-firefox-mweo40ap-2/log
              - listitem [ref=f1e141]:
                - link "Config" [ref=f1e142] [cursor=pointer]:
                  - /url: /topics/r1b-keyed-firefox-mweo40ap-2/config
              - listitem [ref=f1e143]:
                - link "Consumers 0" [ref=f1e144] [cursor=pointer]:
                  - /url: /topics/r1b-keyed-firefox-mweo40ap-2/consumers
                  - text: Consumers
                  - generic [ref=f1e145]: "0"
              - listitem [ref=f1e146]:
                - link "Producers 0" [ref=f1e147] [cursor=pointer]:
                  - /url: /topics/r1b-keyed-firefox-mweo40ap-2/producers
                  - text: Producers
                  - generic [ref=f1e148]: "0"
          - generic [ref=f1e150]:
            - generic [ref=f1e151]:
              - heading "No records yet" [level=3] [ref=f1e156]
              - generic [ref=f1e157]: Nothing has been written to this topic, so there is nothing to read, and no group or producer has touched it. The first record lands at offset 0 of partition 0.
              - button "Produce a record" [ref=f1e159]
            - generic [ref=f1e160]:
              - heading "Or from your app" [level=2] [ref=f1e161]
              - paragraph [ref=f1e162]: The first record lands at offset 0 of partition 0.
              - generic [ref=f1e163]: "echo '{\"order_id\":\"ord-88412\",\"error\":\"payment declined\"}' \\ | kcat -P -b 127.0.0.1:39092 -t r1b-keyed-firefox-mweo40ap-2 -k ord-88412"
              - button "Copy" [ref=f1e165]
      - generic [ref=f1e167]:
        - heading "Jump to a topic or group" [level=2] [ref=f1e168]
        - paragraph [ref=f1e169]: Type a name, then Enter
  - region "Notifications alt+T"
```

# Test source

```ts
  1   | // Slice R1b: Create topic (a page), and the write overlays of a topic (Produce, Add partitions, Delete records, Delete
  2   | // topic), driven with the mouse and the keyboard; every write is checked by what the broker then says.
  3   | import { choose, expect, overflowX, test } from "./fixtures";
  4   | import { call, fresh, get, produce, setConfig, topic } from "./r1b-helpers";
  5   | 
  6   | const dialogUrl = (t: string, d: string, extra = "") => `/topics/${encodeURIComponent(t)}?dialog=${d}${extra}`;
  7   | 
  8   | test.describe("Create topic", () => {
  9   |   test("three steps, the server's live check, Check only, then Create: the topic is there", async ({ page, request, baseURL }, info) => {
  10  |     const name = fresh(info, "made");
  11  |     await page.goto("/topics/new");
  12  |     await expect(page.locator("h1")).toHaveText("Create topic");
  13  |     await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Topics");
  14  |     const nameField = page.getByTestId("name");
  15  |     await expect(nameField).toBeFocused();
  16  |     await page.keyboard.type(name);
  17  |     // The partition count is the broker's default, then Kafka's validate_only answers.
  18  |     await expect(page.getByTestId("partitions")).toHaveValue(/^\d+$/);
  19  |     await expect(page.getByTestId("live-check")).toContainText("Kafka would take this", { timeout: 10_000 });
  20  |     await expect(page.getByTestId("the-call")).toContainText(name);
  21  |     await page.getByTestId("partitions").fill("3");
  22  |     await page.getByTestId("template").getByText("Compacted state").click();
  23  |     await expect(page.getByTestId("terminal")).toContainText("cleanup.policy=compact");
  24  |     for (const w of [1024, 1440]) {
  25  |       await page.setViewportSize({ width: w, height: 900 });
  26  |       expect(await overflowX(page), `overflow at ${w}`).toBe(0);
  27  |     }
  28  |     await page.getByTestId("next-config").click();
  29  |     await expect(page).toHaveURL(/step=config/);
  30  |     await expect(page.getByTestId("cleanup").getByRole("radio", { name: /^compact/ })).toHaveAttribute("aria-checked", "true");
  31  |     await page.getByTestId("retention").fill("3600000");
  32  |     await expect(page.getByText("= 1 h").or(page.getByText(/= 1 hour/))).toBeVisible();
  33  |     await page.getByTestId("next-review").click();
  34  |     await expect(page).toHaveURL(/step=review/);
  35  |     await expect(page.getByTestId("review")).toContainText(name);
  36  |     // Back returns to the step before, with what was typed.
  37  |     await page.goBack();
  38  |     await expect(page.getByTestId("retention")).toHaveValue("3600000");
  39  |     await page.goForward();
  40  |     await page.getByTestId("check-only").click();
  41  |     await expect(page.getByTestId("checked")).toContainText("Every check passed for 3 partitions");
  42  |     expect((await get(request, `/api/v1/topics/${name}/delete`)).status, "Check only makes nothing").toBe(404);
  43  |     await page.getByTestId("create-submit").click();
  44  |     await expect(page.getByTestId("created")).toContainText(`Created ${name}.`);
  45  |     await expect(page.locator("h1")).toHaveText(name);
  46  |     const made = await get(request, `/api/v1/topics/${name}/config`);
  47  |     expect(made.status).toBe(200);
  48  |     expect(made.body.cleanup).toBe("compact");
  49  |     expect(made.body.rows.find((r: { name: string }) => r.name === "retention.ms").value).toBe("3600000");
  50  |     await page.getByTestId("created-produce").click();
  51  |     await expect(page).toHaveURL(new RegExp(`/topics/${name}\\?dialog=produce`));
  52  |     await expect(page.getByTestId("dialog-produce")).toBeVisible();
  53  |     void baseURL;
  54  |   });
  55  | 
  56  |   test("a name Kafka forbids, a name that is taken or collides: said under the field, and Next stays shut", async ({ page }) => {
  57  |     await page.goto("/topics/new");
  58  |     const nameField = page.getByTestId("name");
  59  |     await nameField.fill("bad name!");
  60  |     await expect(page.getByTestId("field-error").first()).toContainText("ASCII letters and digits");
  61  |     await expect(page.getByTestId("next-config")).toBeDisabled();
  62  |     await nameField.fill("orders");
  63  |     await expect(page.getByTestId("field-error").first()).toHaveText("A topic has this name already.");
  64  |     await expect(page.getByTestId("next-config")).toBeDisabled();
  65  |     await nameField.fill("payments_settled");
  66  |     await expect(page.getByTestId("field-error").first()).toHaveText("This name collides with the topic payments.settled.");
  67  |     await expect(page.getByTestId("next-config")).toBeDisabled();
  68  |   });
  69  | 
  70  |   test("a topic made while the form is open: Kafka refuses with its own words, and the way on", async ({ page, request, baseURL }, info) => {
  71  |     const name = fresh(info, "race");
  72  |     await page.goto("/topics/new");
  73  |     await page.getByTestId("name").fill(name);
  74  |     await expect(page.getByTestId("live-check")).toContainText("Kafka would take this", { timeout: 10_000 });
  75  |     await page.getByTestId("next-config").click();
  76  |     await page.getByTestId("next-review").click();
  77  |     await topic(request, baseURL!, name);
  78  |     await page.getByTestId("create-submit").click();
  79  |     const refused = page.getByTestId("create-refused");
  80  |     await expect(refused.getByTestId("refusal-code")).toHaveText("TOPIC_ALREADY_EXISTS (36)");
  81  |     await expect(refused.getByTestId("refusal-message")).toContainText(name);
  82  |     await expect(refused.getByTestId("the-answer")).toContainText("error_code");
  83  |     await refused.getByRole("link", { name: "Open the existing topic" }).click();
  84  |     await expect(page).toHaveURL(new RegExp(`/topics/${name}$`));
  85  |   });
  86  | });
  87  | 
  88  | test.describe("Produce", () => {
  89  |   test("a keyed record with a header to a chosen partition: the offset the log chose, then another", async ({ page, request, baseURL }, info) => {
  90  |     const name = fresh(info, "keyed");
  91  |     await topic(request, baseURL!, name, 3);
  92  |     await page.goto(`/topics/${name}`);
> 93  |     await page.goto(dialogUrl(name, "produce"));
      |                ^ Error: goto: NS_BINDING_ABORTED
  94  |     const d = page.getByTestId("dialog-produce");
  95  |     await expect(d).toBeVisible();
  96  |     await choose(page, d.getByTestId("partition"), "2");
  97  |     await d.getByTestId("key").fill("ord-1");
  98  |     await d.getByTestId("value").fill('{"order_id":"ord-1","qty":2}');
  99  |     await expect(d.getByTestId("json-hint")).toContainText("Valid JSON");
  100 |     await d.getByTestId("value").fill('{"order_id":');
  101 |     await expect(d.getByTestId("json-hint")).toContainText("Not valid JSON");
  102 |     await d.getByTestId("value").fill('{"order_id":"ord-1"}');
  103 |     await d.getByRole("textbox", { name: "Header name 1" }).fill("content-type");
  104 |     await d.getByRole("textbox", { name: "Header value 1" }).fill("application/json");
  105 |     await d.getByTestId("add-header").click();
  106 |     await expect(d.getByTestId("header-row")).toHaveCount(2);
  107 |     await d.getByRole("button", { name: "Remove header 2" }).click();
  108 |     await expect(d.getByTestId("header-row")).toHaveCount(1);
  109 |     await d.getByTestId("acks").getByRole("radio", { name: "1", exact: true }).click();
  110 |     await expect(d.getByTestId("the-call")).toContainText("acks: 1");
  111 |     await expect(d.getByTestId("the-call")).toContainText("content-type");
  112 |     await expect(d.getByTestId("the-call")).toContainText("index: 2");
  113 |     // The teaching folds away; open, it says Kafka's words.
  114 |     await d.getByTestId("teaching-toggle").click();
  115 |     await expect(d.getByTestId("teach")).toContainText("Kafka says");
  116 |     await d.getByTestId("send").click();
  117 |     await expect(d.getByTestId("produced")).toContainText(`${name}-2`);
  118 |     await expect(d.getByTestId("produced-offset")).toHaveText("0");
  119 |     await d.getByTestId("produce-another").click();
  120 |     await d.getByTestId("value").fill("second");
  121 |     await d.getByTestId("send").click();
  122 |     await expect(d.getByTestId("produced-offset")).toHaveText("1");
  123 |     // The broker has both: a third lands at offset 2 of the same partition.
  124 |     expect(await produce(request, baseURL!, name, "ord-1", "third", "2")).toBe(2);
  125 |     await page.keyboard.press("Escape");
  126 |     await expect(d).toBeHidden();
  127 |     await expect(page).not.toHaveURL(/dialog=/);
  128 |   });
  129 | 
  130 |   test("a keyless record is accepted on a delete topic, and Back closes the dialog", async ({ page, request, baseURL }, info) => {
  131 |     const name = fresh(info, "keyless");
  132 |     await topic(request, baseURL!, name, 1);
  133 |     await page.goto(`/topics/${name}`);
  134 |     await page.getByTestId("act-produce").click();
  135 |     await expect(page).toHaveURL(/dialog=produce/);
  136 |     const d = page.getByTestId("dialog-produce");
  137 |     await d.getByTestId("value").fill("no key here");
  138 |     await d.getByTestId("send").click();
  139 |     await expect(d.getByTestId("produced")).toBeVisible();
  140 |     await expect(d.getByTestId("produced-partition")).toHaveText("0");
  141 |     await page.goBack();
  142 |     await expect(d).toBeHidden();
  143 |     await expect(page).toHaveURL(new RegExp(`/topics/${name}$`));
  144 |   });
  145 | 
  146 |   test("a keyless record to a compacted topic: refused INVALID_RECORD in Kafka's words, the key field says why", async ({ page, request, baseURL }, info) => {
  147 |     const name = fresh(info, "compacted");
  148 |     await topic(request, baseURL!, name, 1, "compact");
  149 |     await page.goto(dialogUrl(name, "produce"));
  150 |     const d = page.getByTestId("dialog-produce");
  151 |     await expect(d.getByText("required on this topic")).toBeVisible();
  152 |     await d.getByTestId("value").fill("v");
  153 |     await d.getByTestId("send").click();
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
```