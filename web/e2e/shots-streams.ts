// Screenshots of streams groups (KIP-1071) on the Groups pages, light and dark, in Firefox at 1440 (and 1024), for a person
// to look at. Against a console whose broker has a running streams group and a not-ready one (streams-groups.spec.ts makes
// them, or ./tests/streams.ts by hand).
//   bun --bun shots-streams.ts <console url> <out dir> <group> <not-ready group>
import { firefox } from "@playwright/test";
const [url = "http://127.0.0.1:39652", out = "shots", group = "wordcount-firefox", lonely = "lonely-firefox"] = process.argv.slice(2);
const pages: [string, string][] = [
  [`/groups?q=${encodeURIComponent(group.split("-")[0]!)}`, "streams-list"],
  ["/groups", "groups-list"],
  [`/groups/${group}`, "streams-group"],
  [`/groups/${group}/lag`, "streams-lag"],
  [`/groups/${lonely}`, "streams-not-ready"],
];
const b = await firefox.launch();
for (const scheme of ["light", "dark"] as const) {
  for (const width of [1440, 1024]) {
    const ctx = await b.newContext({ viewport: { width, height: 900 }, colorScheme: scheme });
    await ctx.addInitScript((s) => localStorage.setItem("doze-kafka.theme", s), scheme);
    const p = await ctx.newPage();
    for (const [path, file] of pages) {
      await p.goto(url + path);
      await p.locator("h1").waitFor();
      await p.waitForLoadState("networkidle");
      await p.waitForTimeout(600);
      await p.screenshot({ path: `${out}/${file}.${scheme}.${width}.png`, fullPage: true });
    }
    await ctx.close();
  }
}
await b.close();
console.log("shots in", out);
