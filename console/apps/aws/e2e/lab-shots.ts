// Screenshots of the Lab's two tabs, light and dark, at 1440 x 900 (the whole page), for a person to look at. Against a doze-aws that is
// running with some resources made (a queue, a table, a topic, a bucket, a secret):
//   bun --bun e2e/lab-shots.ts <out dir> <console url> [browser,browser]
// It sends two calls from the page, so the history and the answer are drawn, and saves one input.
import { mkdirSync } from "node:fs";
import { chromium, firefox, type Page, webkit } from "@playwright/test";

const [out = "shots", url = "http://127.0.0.1:4567", only = "chromium"] = process.argv.slice(2);
const engines = { firefox, webkit, chromium };

async function settle(p: Page) {
  await p.evaluate(() => document.fonts.ready);
  await p.waitForTimeout(500);
}

for (const name of only.split(",") as (keyof typeof engines)[]) {
  const b = await engines[name].launch();
  mkdirSync(out, { recursive: true });
  for (const mode of ["light", "dark"] as const) {
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    await ctx.addInitScript((m) => localStorage.setItem("doze-aws.theme", m), mode);
    const p = await ctx.newPage();
    await p.goto(`${url}/lab`);
    await p.getByTestId("lab-recipe-cards").waitFor();
    await settle(p);
    await p.screenshot({ path: `${out}/${name}-rehearse.${mode}.png`, fullPage: true });
    // A recipe opens the editor with its input; send it, then a second call from the model's example.
    await p.getByTestId("lab-recipe-sqs-send").getByTestId("lab-recipe-open").click();
    await p.getByTestId("lab-send").click();
    await p.getByTestId("lab-answer").waitFor();
    await p.getByTestId("lab-save-open").click();
    await p.getByTestId("lab-save-name").fill("Ping the orders queue");
    await p.getByTestId("lab-save-confirm").click();
    await p.goto(`${url}/lab?tab=send&api=sqs&op=ListQueues`);
    await p.getByTestId("lab-send").click();
    await p.getByTestId("lab-answer").waitFor();
    await settle(p);
    await p.screenshot({ path: `${out}/${name}-send.${mode}.png`, fullPage: true });
    // The secret recipe, answered, with its value hidden.
    await p.goto(`${url}/lab`);
    await p.getByTestId("lab-recipe-cards").waitFor();
    await p.getByTestId("lab-recipe-secrets-get").waitFor();
    const secret = p.getByTestId("lab-recipe-secrets-get");
    if ((await secret.count()) > 0) {
      await secret.getByTestId("lab-recipe-open").click();
      await p.getByTestId("lab-send").click();
      await p.getByTestId("lab-reveal").waitFor();
      await settle(p);
      await p.screenshot({ path: `${out}/${name}-secret.${mode}.png`, fullPage: true });
    }
    await ctx.close();
  }
  await b.close();
}
