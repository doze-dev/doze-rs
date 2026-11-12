// A throwaway look at one page: bun tools/ux/probe.ts --url http://127.0.0.1:<port> <path> "<js>" ["<js>"...]
// Loads the path in Chrome, evaluates each expression in turn and prints its value (and any console problem). For debugging a scenario.
import { Browser } from "./cdp.ts";
import { Page } from "./page.ts";

const args = process.argv.slice(2);
const u = args.indexOf("--url");
const base = args[u + 1];
const rest = args.filter((_, i) => i !== u && i !== u + 1);
const [path, ...exprs] = rest;
const b = await Browser.launch();
try {
  const tab = await b.tab();
  const p = new Page(b, tab.sessionId, base);
  await p.init();
  await p.goto(path);
  for (const e of exprs) {
    if (e.startsWith("key:")) { await p.key(e.slice(4)); console.log("key", e.slice(4)); continue; }
    if (e.startsWith("type:")) { await p.type(e.slice(5)); console.log("type", e.slice(5)); continue; }
    if (e.startsWith("click:")) { await p.click(e.slice(6)); console.log("click", e.slice(6)); continue; }
    if (e.startsWith("hover:")) { await p.hover(e.slice(6)); console.log("hover", e.slice(6)); continue; }
    if (e.startsWith("sleep:")) { await Bun.sleep(Number(e.slice(6))); continue; }
    if (e.startsWith("shot:")) { console.log(await p.shot(e.slice(5))); continue; }
    try { console.log(JSON.stringify(await p.eval(e))); } catch (err) { console.log("ERR", (err as Error).message); }
  }
  if (p.logs.length) console.log("LOGS", p.logs);
} finally {
  b.kill();
  await Bun.sleep(1200);
}
