// Screenshots of the gallery, Home and Connect in Firefox and WebKit (and Chromium) for a person to look at.
//   bun --bun shots.ts <console url> <out dir>
import { chromium, firefox, webkit } from "@playwright/test";
const [url = "http://127.0.0.1:29094", out = "shots"] = process.argv.slice(2);
for (const [name, type] of [["firefox", firefox], ["webkit", webkit], ["chromium", chromium]] as const) {
  const b = await type.launch();
  for (const width of [1440, 1024]) {
    const p = await b.newPage({ viewport: { width, height: 900 } });
    for (const [path, file] of [["/", "home"], ["/connect", "connect"], ["/gallery", "gallery"]] as const) {
      await p.goto(url + path);
      await p.locator("h1").waitFor();
      await p.waitForLoadState("networkidle");
      await p.waitForTimeout(400);
      await p.screenshot({ path: `${out}/${file}.${name}.${width}.png`, fullPage: true });
    }
    await p.close();
  }
  await b.close();
}
console.log("shots in", out);
