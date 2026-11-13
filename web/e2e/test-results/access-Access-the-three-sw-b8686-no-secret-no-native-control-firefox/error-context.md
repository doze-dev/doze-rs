# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: access.spec.ts >> Access: the three switches, users, ACLs, the console card and the start warnings; no secret, no native control
- Location: tests/access.spec.ts:6:1

# Error details

```
Error: tables wider than their box at 1024

expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "ACLs",
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
            - heading "Access" [level=1] [ref=e94]
            - paragraph [ref=e95]:
              - generic [ref=e96]: Open
              - text: Any client on this machine may connect and do anything. Everything below is read from the broker; no password, key or salt is ever shown.
        - generic [ref=e99]:
          - generic [ref=e100]:
            - generic [ref=e101]:
              - generic [ref=e102]: Authentication
              - generic [ref=e103]: "Off"
              - generic [ref=e105]: Any client may connect and send requests; nobody logs in.
            - generic [ref=e107]:
              - generic [ref=e108]:
                - term [ref=e109]: Mechanisms
                - definition [ref=e110]:
                  - generic [ref=e111]: none
                  - generic [ref=e112]: "--sasl-mechanisms sets them"
              - generic [ref=e113]:
                - term [ref=e114]: Users
                - definition [ref=e115]:
                  - generic [ref=e116]: 0 from flags, 0 stored
          - generic [ref=e118]:
            - generic [ref=e119]:
              - generic [ref=e120]: Authorizer
              - generic [ref=e121]: Allows by default
              - generic [ref=e123]: A resource no ACL covers is open to everyone; an ACL that matches still decides.
            - generic [ref=e125]:
              - generic [ref=e126]:
                - term [ref=e127]: Default
                - definition [ref=e128]:
                  - generic [ref=e129]: "--acl-default allow"
              - generic [ref=e131]:
                - term [ref=e132]: ACLs
                - definition [ref=e133]:
                  - generic [ref=e134]: "0"
                  - generic [ref=e135]: 0 allow, 0 deny
              - generic [ref=e136]:
                - term [ref=e137]: Super users
                - definition [ref=e138]:
                  - generic [ref=e139]: none
                  - generic [ref=e140]: "--acl-super-user names them"
          - generic [ref=e141]:
            - generic [ref=e142]:
              - generic [ref=e143]: Encryption
              - generic [ref=e144]: Plaintext
              - generic [ref=e146]: The Kafka listener speaks plaintext.
            - generic [ref=e149]:
              - term [ref=e150]: Listener
              - definition [ref=e151]:
                - generic [ref=e152]: 127.0.0.1:39392
                - generic [ref=e153]: "loopback: this machine only"
        - region "Users" [ref=e154]:
          - generic [ref=e156]:
            - heading "Users" [level=2] [ref=e157]
            - paragraph [ref=e158]: "What DescribeUserScramCredentials returns: a name, a mechanism and an iteration count. No password, salt or key is ever shown. A user from the flags or a file is re-applied at every start and cannot be changed over the API."
          - generic [ref=e159]:
            - table "Users" [ref=e160]:
              - rowgroup [ref=e166]:
                - row [ref=e167]:
                  - columnheader "User" [ref=e168]:
                    - button "User" [ref=e169]
                  - columnheader "From" [ref=e172]:
                    - button "From" [ref=e173]
                  - columnheader "Mechanisms" [ref=e176]:
                    - button "Mechanisms" [ref=e177]
                  - columnheader "Iterations" [ref=e180]:
                    - button "Iterations" [ref=e181]
              - rowgroup
            - generic [ref=e184]: Nobody logs in on this broker, so there are no users.
        - region "ACLs" [ref=e185]:
          - generic [ref=e186]:
            - generic [ref=e187]:
              - heading "ACLs" [level=2] [ref=e188]
              - paragraph [ref=e189]: What DescribeAcls returns. A DENY wins over any ALLOW; an ALLOW of Read, Write, Delete or Alter also allows Describe.
            - button "Create ACL" [ref=e191]
          - generic [ref=e192]:
            - textbox "Filter ACLs" [ref=e194]:
              - /placeholder: Name, principal, host…
            - combobox "Resource type" [ref=e195]:
              - generic: Every resource type
            - radiogroup "Permission" [ref=e196]:
              - radio "Both" [checked] [ref=e197]
              - radio "Allow" [ref=e198]
              - radio "Deny" [ref=e199]
            - generic [ref=e200]: 0 ACLs
          - generic [ref=e201]:
            - table "ACLs" [ref=e202]:
              - rowgroup [ref=e210]:
                - row [ref=e211]:
                  - columnheader "Resource" [ref=e212]:
                    - button "Resource" [ref=e213]
                  - columnheader "Pattern" [ref=e216]:
                    - button "Pattern" [ref=e217]
                  - columnheader "Principal @ host" [ref=e220]:
                    - button "Principal @ host" [ref=e221]
                  - columnheader "Operation" [ref=e224]:
                    - button "Operation" [ref=e225]
                  - columnheader "Permission" [ref=e228]:
                    - button "Permission" [ref=e229]
                  - columnheader "Delete" [ref=e232]
              - rowgroup
            - generic [ref=e234]: No ACL yet. With --acl-default allow, every resource is open until an ACL says otherwise.
        - generic [ref=e235]:
          - generic [ref=e236]:
            - generic [ref=e237]:
              - generic [ref=e238]: The console
              - generic [ref=e239]: It reads and writes the broker the way a client does, as one more principal, and it has no login of its own.
            - generic [ref=e241]:
              - generic [ref=e242]:
                - term [ref=e243]: Principal
                - definition [ref=e244]:
                  - generic [ref=e245]: User:__console
                  - generic [ref=e246]: "reserved: no user, credential or --acl-super-user may take the name"
              - generic [ref=e247]:
                - term [ref=e248]: Rights
                - definition [ref=e249]:
                  - generic [ref=e250]: Super user
                  - generic [ref=e251]: so the ACLs above never stop the console
              - generic [ref=e252]:
                - term [ref=e253]: Listens on
                - definition [ref=e254]:
                  - generic [ref=e255]: 127.0.0.1:39394
                  - generic [ref=e256]: loopback; with SASL or TLS on, anywhere else is refused at start unless --console-unprotected
              - generic [ref=e257]:
                - term [ref=e258]: Allowed hosts
                - definition [ref=e259]:
                  - generic [ref=e260]: IP addresses and localhost
                  - generic [ref=e261]: none added with --console-allow-host
              - generic [ref=e262]:
                - term [ref=e263]: Writes
                - definition [ref=e264]:
                  - generic [ref=e265]: Same origin only
          - generic [ref=e266]:
            - generic [ref=e267]:
              - generic [ref=e268]: What the broker said at start
              - generic [ref=e269]: Three warnings, each only when its condition holds.
            - list [ref=e271]:
              - listitem [ref=e272]:
                - generic [ref=e276]:
                  - generic [ref=e277]: Console served beyond loopback with SASL or TLS on
                  - generic [ref=e278]:
                    - text: The console is on
                    - code [ref=e279]: 127.0.0.1:39394
                - generic [ref=e280]: not the case
              - listitem [ref=e281]:
                - generic [ref=e285]:
                  - generic [ref=e286]: SASL without TLS beyond loopback
                  - generic [ref=e287]: SASL is off
                - generic [ref=e288]: not the case
              - listitem [ref=e289]:
                - generic [ref=e293]:
                  - generic [ref=e294]:
                    - text: SASL on with
                    - code [ref=e295]: "--acl-default allow"
                  - generic [ref=e296]: SASL is off
                - generic [ref=e297]: not the case
      - generic [ref=e298]:
        - heading "Jump to a topic or group" [level=2] [ref=e299]
        - paragraph [ref=e300]: Type a name, then Enter
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