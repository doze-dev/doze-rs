// Screenshots of the Lab's states in Firefox and WebKit, 1440 and 1024, light and dark, for a person to look at.
//   bun --bun shots-lab.ts <console url> <out dir> [states...]
// States: `first` (a first visit: How the Lab works open), `page` (a returning visitor: folded), `dialog` (a recipe in the New
// rehearsal dialog), `lesson` (the dialog with the lesson open), `sheet` (the newest rule's sheet), `live` (Live filtered to
// it), `stop` (the Stop all dialog). The broker needs traffic for the recipes to name real things.
import { firefox, webkit, type Page } from "@playwright/test";
const [url = "http://127.0.0.1:29994", out = "shots", ...only] = process.argv.slice(2);
const states = only.length ? only : ["first", "page", "dialog", "lesson", "sheet", "live", "stop"];

async function settle(p: Page) {
  await p.locator("h1").first().waitFor();
  await p.waitForLoadState("networkidle");
  await p.waitForTimeout(600);
}

async function newest(p: Page): Promise<number | undefined> {
  const lab = await (await p.request.get(url + "/api/v1/lab", { headers: { "X-Console": "1", "Sec-Fetch-Site": "same-origin" } })).json();
  return lab.home.active.at(-1)?.id;
}

for (const [name, type] of [["firefox", firefox], ["webkit", webkit]] as const) {
  const b = await type.launch();
  for (const scheme of ["light", "dark"] as const) {
    for (const width of [1440, 1024]) {
      for (const state of states) {
        const ctx = await b.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
        await ctx.addInitScript((first) => (first ? localStorage.removeItem("doze:lab-guide") : localStorage.setItem("doze:lab-guide", "folded")), state === "first");
        const p = await ctx.newPage();
        const file = `${out}/lab-${state}.${name}.${width}.${scheme}.png`;
        const id = state === "sheet" || state === "live" ? await newest(p) : undefined;
        const path = {
          first: "/lab",
          page: "/lab",
          dialog: "/lab?new=fail&recipe=fail-leader",
          lesson: "/lab?new=slow&recipe=slow-produce",
          sheet: `/lab?rule=${id}`,
          live: `/live?lab=${id}`,
          stop: "/lab?stop=all",
        }[state];
        if (!path || ((state === "sheet" || state === "live") && !id)) {
          await ctx.close();
          continue;
        }
        await p.goto(url + path);
        await settle(p);
        if (state === "lesson") {
          await p.getByTestId("lab-lesson-toggle").click();
          await p.waitForTimeout(400);
        }
        await p.screenshot({ path: file, fullPage: state === "first" || state === "page" });
        await ctx.close();
      }
    }
  }
  await b.close();
}
console.log("shots in", out);
