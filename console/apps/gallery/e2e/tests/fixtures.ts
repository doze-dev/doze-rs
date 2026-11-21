import { test as base, expect, type Page } from "@playwright/test";
import { at, type Look } from "../looks";

/** Every test fails on a console error or an uncaught exception in the page (the owner's bar: no console errors). */
export const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on("console", (m) => {
        // A font download that the next navigation cut short (Firefox: NS_BINDING_ABORTED, 0x804b0002) is the test moving
        // faster than a person, not a fault of the page.
        if (m.type() === "error" && !/downloadable font: download failed.*status=2152398850/.test(m.text())) errors.push(`console: ${m.text()}`);
      });
      page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
      await use(errors);
      expect(errors, "no console errors").toEqual([]);
    },
    { auto: true },
  ],
});

export { expect };

/** The page ground each look must paint: the theme file's --background, as the browser reports it. */
export const GROUND: Record<string, string> = {
  "kafka-light": "rgb(250, 249, 245)",
  "kafka-dark": "rgb(18, 17, 16)",
  "aws-light": "rgb(247, 249, 252)",
  "aws-dark": "rgb(15, 22, 38)",
};
export const tag = (look: Look) => `${look.theme}-${look.mode}`;

/** Opens a gallery page under a look and waits until it is drawn in it. */
export async function open(page: Page, path: string, look: Look) {
  await page.goto(at(path, look));
  await expect(page.locator("h1").first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-product", look.theme);
  await expect(page.locator("html")).toHaveClass(new RegExp(`\\b${look.mode}\\b`));
}

/** The page scrolls sideways by this many pixels (0 is the rule). */
export async function overflowX(page: Page): Promise<number> {
  // Never below 0: with classic scrollbars (Chromium on Linux) the client width can exceed the scroll width by the bar.
  return page.evaluate(() => Math.max(0, document.documentElement.scrollWidth - document.documentElement.clientWidth));
}

/** Tab, as a person presses it. Safari's Tab skips buttons unless Option is held (its default), so there it is Option+Tab. */
export const tabKey = (browserName: string) => (browserName === "webkit" ? "Alt+Tab" : "Tab");

/** Presses Tab until `target` has the focus (at most `most` times): the focus arrives by the keyboard, as :focus-visible needs. */
export async function tabTo(page: Page, browserName: string, target: ReturnType<Page["locator"]>, most = 60) {
  for (let i = 0; i < most; i++) {
    await page.keyboard.press(tabKey(browserName));
    if (await target.evaluate((el) => el === document.activeElement)) return;
  }
  throw new Error(`Tab did not reach the target in ${most} presses`);
}

/** Whether the focused element shows a ring: a box-shadow or an outline that is there and not transparent. A control eases its
 * ring in (transition-all), so the ring is waited for, for as long as a transition may take. */
export async function ringOf(target: ReturnType<Page["locator"]>): Promise<{ shadow: string; outline: string; visible: boolean }> {
  let last = await ringNow(target);
  // Up to 3 s: under CI load a transition (and WebKit's focus) can start late; a ring that is there returns at once.
  for (const deadline = Date.now() + 3000; Date.now() < deadline && !last.visible; ) {
    await target.page().waitForTimeout(50);
    last = await ringNow(target);
  }
  return last;
}

async function ringNow(target: ReturnType<Page["locator"]>): Promise<{ shadow: string; outline: string; visible: boolean }> {
  return target.evaluate((el) => {
    const s = getComputedStyle(el);
    const clear = (c: string) => /rgba\([^)]*,\s*0\)$|transparent/.test(c);
    const shadow = s.boxShadow;
    // A ring is a spread shadow in a colour: at least one layer of the box-shadow that is not fully transparent.
    const shadowOn = shadow !== "none" && shadow.split(/,(?![^(]*\))/).some((layer) => !/rgba\([^)]*,\s*0\)/.test(layer) && /\d+px\s+\d+px\s+\d+px\s+[1-9]/.test(layer.replace(/rgba?\([^)]*\)|oklab\([^)]*\)|color\([^)]*\)/g, "")));
    const outlineOn = s.outlineStyle !== "none" && Number.parseFloat(s.outlineWidth) > 0 && !clear(s.outlineColor);
    return { shadow, outline: `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}`, visible: shadowOn || outlineOn };
  });
}
