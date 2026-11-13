# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Produce >> a keyed record with a header to a chosen partition: the offset the log chose, then another
- Location: tests/topic-writes.spec.ts:89:3

# Error details

```
Error: click: Error: strict mode violation: getByTestId('dialog-produce').getByTestId('acks').getByRole('radio', { name: '1' }) resolved to 2 elements:
    1) <button role="radio" type="button" tabindex="-1" data-state="on" data-spacing="0" aria-checked="true" data-variant="outline" data-slot="toggle-group-item" data-radix-collection-item="" class="inline-flex items-center justify-center gap-2 rounded-md text-sm font-medium whitespace-nowrap transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring…>all (-1)</button> aka getByRole('radio', { name: 'all (-1)' })
    2) <button role="radio" type="button" tabindex="-1" data-state="off" data-spacing="0" aria-checked="false" data-variant="outline" data-slot="toggle-group-item" data-radix-collection-item="" class="inline-flex items-center justify-center gap-2 rounded-md text-sm font-medium whitespace-nowrap transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ri…>1</button> aka getByRole('radio', { name: '1', exact: true })

Call log:
  - waiting for getByTestId('dialog-produce').getByTestId('acks').getByRole('radio', { name: '1' })

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
                      - generic: "13"
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
                - heading [level=1]: r1b-keyed-firefox-mwe873c7-3
              - generic:
                - button: Add partitions
                - button: Delete records
                - button: Produce
                - button: Delete topic
          - navigation:
            - link:
              - /url: /topics/r1b-keyed-firefox-mwe873c7-3
              - text: Messages
            - link:
              - /url: /topics/r1b-keyed-firefox-mwe873c7-3/log
              - text: Log
            - link:
              - /url: /topics/r1b-keyed-firefox-mwe873c7-3/config
              - text: Config
            - link:
              - /url: /topics/r1b-keyed-firefox-mwe873c7-3/consumers
              - text: Consumers
            - link:
              - /url: /topics/r1b-keyed-firefox-mwe873c7-3/producers
              - text: Producers
          - paragraph:
            - text: Messages is being rebuilt (slice R1a).
            - link:
              - /url: /classic/topics/r1b-keyed-firefox-mwe873c7-3
              - text: Open it in the classic console
            - text: .
        - generic:
          - heading [level=2]: Jump to a topic or group
          - paragraph: Type a name, then Enter
    - region "Notifications alt+T"
  - dialog "Produce to r1b-keyed-firefox-mwe873c7-3" [active] [ref=f1e2]:
    - generic [ref=f1e3]:
      - heading [level=2] [ref=f1e4]:
        - text: Produce to
        - code [ref=f1e5]: r1b-keyed-firefox-mwe873c7-3
      - paragraph [ref=f1e6]: One record. The call below is the one that will be sent; the broker chooses the offset.
    - generic [ref=f1e8]:
      - generic [ref=f1e9]:
        - generic [ref=f1e10]:
          - generic [ref=f1e11]: Partition
          - combobox "Partition" [ref=f1e12]:
            - generic: "2"
          - combobox [aria-hidden] [ref=f1e13]
          - paragraph [ref=f1e14]: Auto, or 0 to 2. Auto sends a key where Kafka's default partitioner does.
        - generic [ref=f1e15]:
          - generic [ref=f1e16]:
            - text: Key
            - generic [ref=f1e17]: optional on this topic
          - textbox "Key optional on this topic" [ref=f1e18]: ord-1
      - generic [ref=f1e19]:
        - generic [ref=f1e20]: Value
        - textbox "Value" [ref=f1e21]:
          - /placeholder: "{\"order_id\": \"ord-1\"}"
          - text: "{\"order_id\":\"ord-1\"}"
        - paragraph [ref=f1e22]: Valid JSON. The broker stores the bytes as typed.
      - group "Headers up to 8" [ref=f1e23]:
        - generic [ref=f1e25]:
          - textbox "Header name 1" [ref=f1e26]:
            - /placeholder: name
            - text: content-type
          - textbox "Header value 1" [ref=f1e27]:
            - /placeholder: value
            - text: application/json
          - button "Remove header 1" [ref=f1e28]
        - button "Add header" [ref=f1e30]
      - generic [ref=f1e31]:
        - generic [ref=f1e32]: acks
        - radiogroup "acks" [ref=f1e33]:
          - radio "all (-1)" [checked] [ref=f1e34]
          - radio "1" [ref=f1e35]
          - radio "0" [ref=f1e36]
      - button "What Kafka says about these fields" [ref=f1e38]
      - region "The call" [ref=f1e41]:
        - heading "The call" [level=3] [ref=f1e42]
        - generic [ref=f1e49]:
          - generic [ref=f1e50]: Produce v9
          - generic [ref=f1e51]: "acks: -1"
          - generic [ref=f1e52]: "timeout_ms: 30000"
          - generic [ref=f1e53]: topic_data[0]
          - generic [ref=f1e54]: "name: r1b-keyed-firefox-mwe873c7-3"
          - generic [ref=f1e55]: partition_data[0]
          - generic [ref=f1e56]: "index: 0"
          - generic [ref=f1e57]: "records: 1 batch, 1 record"
          - generic [ref=f1e58]: "key: (none)"
          - generic [ref=f1e59]: "value: (none: a deletion)"
      - generic [ref=f1e60]:
        - button "Cancel" [ref=f1e61]
        - button "Send" [ref=f1e62]
    - button "Close" [ref=f1e63]
```

# Test source

```ts
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
  93  |     await page.goto(dialogUrl(name, "produce"));
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
> 109 |     await d.getByTestId("acks").getByRole("radio", { name: "1" }).click();
      |                                                                   ^ Error: click: Error: strict mode violation: getByTestId('dialog-produce').getByTestId('acks').getByRole('radio', { name: '1' }) resolved to 2 elements:
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
  134 |     await page.getByTestId("open-produce").click();
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
```