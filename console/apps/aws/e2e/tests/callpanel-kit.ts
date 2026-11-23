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
