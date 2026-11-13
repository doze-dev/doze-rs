// Helpers of the Settings, Access and Lab specs (slice R4): the broker's Kafka address, a client with traffic, the API with the
// console's proof, and the checks every page of the slice must pass.
import type { APIRequestContext, Page } from "@playwright/test";
import { Kafka, logLevel, type Producer } from "kafkajs";
import { expect, overflowX } from "./fixtures";

/** The Kafka address the console serves (the top bar's), so a spec can make traffic against a seeded or a demo broker. */
export async function kafkaAddress(request: APIRequestContext): Promise<string> {
  const res = await request.get("/api/v1/shell", { headers: { "X-Console": "1" } });
  return (await res.json()).address as string;
}

/** A POST to the console's API as its own page sends it. */
export async function apiPost(request: APIRequestContext, baseURL: string, path: string, body: unknown) {
  const res = await request.post(path, { headers: { "X-Console": "1", Origin: new URL(baseURL).origin, "Content-Type": "application/json" }, data: JSON.stringify(body) });
  return { status: res.status(), body: await res.json() };
}

export async function apiGet(request: APIRequestContext, path: string) {
  const res = await request.get(path, { headers: { "X-Console": "1" } });
  return res.json();
}

/** A producer that keeps writing to `topic` every `ms` until stopped. */
export async function traffic(address: string, clientId: string, topic = "orders", ms = 150) {
  const kafka = new Kafka({ clientId, brokers: [address], logLevel: logLevel.NOTHING, retry: { retries: 0 } });
  const producer: Producer = kafka.producer({ allowAutoTopicCreation: false, retry: { retries: 0 } });
  await producer.connect();
  let on = true;
  let sent = 0;
  const errors: string[] = [];
  const loop = (async () => {
    while (on) {
      try {
        await producer.send({ topic, acks: 1, messages: [{ key: `k${sent}`, value: `lab ${sent}` }] });
        sent++;
      } catch (e) {
        errors.push((e as { type?: string }).type ?? String(e));
      }
      await new Promise((r) => setTimeout(r, ms));
    }
  })();
  return {
    kafka,
    producer,
    errors,
    sent: () => sent,
    async stop() {
      on = false;
      await loop;
      await producer.disconnect().catch(() => {});
    },
  };
}

/** No browser-default control: a native select may exist only as Radix's hidden one (aria-hidden, a pixel wide). */
export async function noNativeSelect(page: Page) {
  const shown = await page.evaluate(() =>
    [...document.querySelectorAll("select, input[type=date], input[type=number], input[type=time], input[type=datetime-local]")].filter((el) => {
      const r = el.getBoundingClientRect();
      return el.getAttribute("aria-hidden") !== "true" && r.width > 1 && r.height > 1;
    }).length,
  );
  expect(shown, "no native select, date or number widget is visible").toBe(0);
}

/** No horizontal overflow at the four widths, and the page's h1 once. */
export async function widths(page: Page) {
  for (const width of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(100);
    expect(await overflowX(page), `overflow at ${width}`).toBeLessThanOrEqual(0);
    // No table scrolls sideways inside its box either: its columns fit.
    const wide = await page.evaluate(() => [...document.querySelectorAll("[data-slot=data-table]")].filter((el) => el.scrollWidth > el.clientWidth + 1).map((el) => el.querySelector("table")?.getAttribute("aria-label")));
    expect(wide, `tables wider than their box at ${width}`).toEqual([]);
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.locator("h1")).toHaveCount(1);
}

/** The page's words: no memory figure anywhere (owner, 2026-11-13). */
export async function noMemory(page: Page) {
  const text = (await page.locator("body").innerText()).toLowerCase();
  for (const word of ["memory", "resident", "rss"]) expect(text, word).not.toContain(word);
}
