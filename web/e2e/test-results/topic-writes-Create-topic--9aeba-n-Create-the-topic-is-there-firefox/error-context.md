# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: topic-writes.spec.ts >> Create topic >> three steps, the server's live check, Check only, then Create: the topic is there
- Location: tests/topic-writes.spec.ts:9:3

# Error details

```
Error: Check only makes nothing

expect(received).toBe(expected) // Object.is equality

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
          - generic: "11"
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
            - navigation "Breadcrumb" [ref=e91]:
              - list [ref=e92]:
                - listitem [ref=e93]:
                  - link "Topics" [ref=e94] [cursor=pointer]:
                    - /url: /topics
            - generic [ref=e96]:
              - heading "Create topic" [level=1] [ref=e97]
              - paragraph [ref=e98]: Check what will be sent, then create. Nothing exists until you do.
          - generic [ref=e99]:
            - tablist "Steps" [ref=e100]:
              - tab "Name and partitions" [ref=e101]
              - tab "Configuration" [ref=e103]
              - tab "3 Review and create" [selected] [ref=e105]:
                - generic [ref=e106]: "3"
                - text: Review and create
            - generic [ref=e107]:
              - tabpanel "3 Review and create" [ref=e112]:
                - heading "What you will get" [level=2] [ref=e113]
                - generic [ref=e114]:
                  - generic [ref=e115]:
                    - term [ref=e116]: Name
                    - definition [ref=e117]:
                      - code [ref=e118]: r1b-made-firefox-mweo3w74-8
                  - generic [ref=e119]:
                    - term [ref=e120]: Partitions
                    - definition [ref=e121]: 3 directories made and flushed before the topic is announced
                  - generic [ref=e122]:
                    - term [ref=e123]: Replication factor
                    - definition [ref=e124]: 1 one broker, said once
                  - generic [ref=e125]:
                    - term [ref=e126]: cleanup.policy
                    - definition [ref=e127]:
                      - code [ref=e128]: compact
                  - generic [ref=e129]:
                    - term [ref=e130]: retention.ms
                    - definition [ref=e131]:
                      - code [ref=e132]: "3600000"
                      - text: (1 h)
                  - generic [ref=e133]:
                    - term [ref=e134]: Everything else
                    - definition [ref=e135]: the broker's defaults, none set on the topic
                - status [ref=e136]:
                  - paragraph [ref=e139]: Checked. Every check passed for 3 partitions, and nothing was created.
                - generic [ref=e140]:
                  - text: What can refuse this call. A name that already exists, a name that collides with one by
                  - code [ref=e141]: .
                  - text: and
                  - code [ref=e142]: _
                  - text: ", a partition count outside 1 to 1,000, a config value Kafka would not accept. Check only runs every one of those checks and creates nothing. If a client makes"
                  - code [ref=e143]: r1b-made-firefox-mweo3w74-8
                  - text: while you read this, the call is refused.
                - generic [ref=e144]:
                  - button "Back" [ref=e145]
                  - button "Check only" [active] [ref=e146]
                  - button "Create topic" [ref=e147]
              - generic [ref=e148]:
                - region "The call" [ref=e149]:
                  - heading "The call" [level=3] [ref=e150]
                  - generic [ref=e157]:
                    - generic [ref=e158]: CreateTopics v7
                    - generic [ref=e159]: topics[0]
                    - generic [ref=e160]: "name: r1b-made-firefox-mweo3w74-8"
                    - generic [ref=e161]: "num_partitions: 3"
                    - generic [ref=e162]: "replication_factor: 1"
                    - generic [ref=e163]: "configs:"
                    - generic [ref=e164]: cleanup.policy = compact
                    - generic [ref=e165]: retention.ms = 3600000
                    - generic [ref=e166]: "validate_only: false"
                - region "Or from a terminal" [ref=e167]:
                  - generic [ref=e168]:
                    - heading "Or from a terminal" [level=3] [ref=e169]
                    - button "Copy" [ref=e170]
                  - generic [ref=e172]: kafka-topics.sh --bootstrap-server 127.0.0.1:39092 \ --create --topic r1b-made-firefox-mweo3w74-8 --partitions 3 \ --config cleanup.policy=compact \ --config retention.ms=3600000
      - generic [ref=e173]:
        - heading "Jump to a topic or group" [level=2] [ref=e174]
        - paragraph [ref=e175]: Type a name, then Enter
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
> 42  |     expect((await get(request, `/api/v1/topics/${name}/delete`)).status, "Check only makes nothing").toBe(404);
      |                                                                                                      ^ Error: Check only makes nothing
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
```