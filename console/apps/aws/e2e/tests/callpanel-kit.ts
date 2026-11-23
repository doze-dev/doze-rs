// The call panel's copy buttons, from a test: the AWS CLI line and the boto3 snippet the preview answers, and the JSON that is sent.
import { expect, type Page } from "@playwright/test";

const BUTTON = { cli: /^(Copy as )?aws cli$/i, boto3: /^(Copy as )?boto3$/i, json: /^Copy JSON$/ } as const;

/** Presses one of the call panel's copy buttons and checks it said so (the button says `Copied`). Where a test may read the
 * clipboard (Chromium only: the other two give a page no read permission) the text that was copied is returned, else `null`. */
export async function copyCall(page: Page, which: keyof typeof BUTTON, panel = "call-panel"): Promise<string | null> {
  const box = page.getByTestId(panel).first();
  const copied = box.getByRole("button", { name: "Copied" });
  // A button that has just been pressed says Copied for a moment: wait it out, so the one pressed now is the only one that does.
  await expect(copied).toHaveCount(0);
  await box.getByRole("button", { name: BUTTON[which] }).click();
  await expect(copied).toHaveCount(1);
  if (page.context().browser()?.browserType().name() !== "chromium") return null;
  return page.evaluate(() => navigator.clipboard.readText());
}

/** `copyCall`, and where the clipboard can be read, what it copied holds `text` (the other two browsers only check it was copied). */
export async function expectCopy(page: Page, which: keyof typeof BUTTON, text: string, panel = "call-panel"): Promise<void> {
  const copied = await copyCall(page, which, panel);
  if (copied !== null) expect(copied).toContain(text);
}

/** The call panel of a form with a long call sits full width under the form (as the boards draw it), and its code reads as drawn:
 * no line of it is blank where the call has none, and no short line is broken in two. */
export async function expectFullWidthCall(page: Page, panel = "call-panel"): Promise<void> {
  const box = page.getByTestId(panel).first();
  const form = page.locator("form").first();
  await expect(box).toBeVisible();
  const [b, f] = await Promise.all([box.boundingBox(), form.boundingBox()]);
  expect(b!.width).toBeGreaterThan(f!.width * 0.9);
  expect(b!.y).toBeGreaterThan(f!.y);
  await expectWholeLines(page, `${panel}-code`);
}

/** Every line of a call's code that is short enough to fit is on one row, and none of its lines is empty (a call has no blank line). */
export async function expectWholeLines(page: Page, code = "call-panel-code"): Promise<void> {
  const lines = await page.getByTestId(code).first().evaluate((pre) =>
    [...pre.children].map((el) => ({ text: el.textContent ?? "", h: (el as HTMLElement).offsetHeight, lh: Number.parseFloat(getComputedStyle(el).lineHeight) })),
  );
  expect(lines.length).toBeGreaterThan(2);
  for (const l of lines) {
    expect(l.text.trim(), "a blank line in the call").not.toBe("");
    if (l.text.length < 60) expect(l.h, `"${l.text}" is broken over lines`).toBeLessThanOrEqual(l.lh + 1);
  }
}
