// The overlay components, driven as a person drives them, under every look: Dialog, Sheet, Popover, the command palette,
// Select and Combobox. Each opens, keeps the keyboard, does its job, closes, and gives the focus back.
import { LOOKS } from "../looks";
import { expect, open, tag, test } from "./fixtures";

for (const look of LOOKS) {
  test.describe(tag(look), () => {
    test("Dialog: opens over the page, Esc closes it to its opener, and its write shows what was done", async ({ page }) => {
      await open(page, "/overlays", look);
      const opener = page.getByTestId("open-dialog");
      await opener.click();
      const dialog = page.getByTestId("dialog");
      await expect(dialog).toBeVisible();
      await expect(dialog).toHaveAttribute("role", "dialog");
      // The page behind cannot take the focus while it is open.
      await expect(page.locator("#root")).toHaveAttribute("inert", "");
      expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
      await page.keyboard.press("Escape");
      await expect(dialog).toBeHidden();
      await expect(opener).toBeFocused();
      await expect(page.locator("#root")).not.toHaveAttribute("inert", "");

      await opener.click();
      const del = page.getByTestId("dialog-delete");
      await expect(del).toBeDisabled();
      await page.getByTestId("dialog-confirm").fill("orders");
      await expect(del).toBeEnabled();
      await del.click();
      await expect(page.getByTestId("dialog-done")).toContainText("orders is deleted");
      await page.getByTestId("write-done").click();
      await expect(dialog).toBeHidden();
      await expect(opener).toBeFocused();
    });

    test("Sheet: a drawer with its own address; Esc and Back close it, and the focus returns to the row's button", async ({ page }) => {
      await open(page, "/overlays", look);
      const opener = page.getByTestId("open-sheet-r1");
      await opener.click();
      const sheet = page.getByTestId("sheet");
      await expect(sheet).toBeVisible();
      await expect(page).toHaveURL(/[?&]record=r1\b/);
      await expect(sheet.getByRole("heading")).toHaveText("clickstream-0001");
      // It is an overlay: the page it was opened from is still there, behind it.
      await expect(page.locator("h1").first()).toHaveText("Overlays");
      await page.keyboard.press("Escape");
      await expect(sheet).toBeHidden();
      await expect(page).not.toHaveURL(/record=/);
      await expect(opener).toBeFocused();

      await opener.click();
      await expect(sheet).toBeVisible();
      await page.goBack();
      await expect(sheet).toBeHidden();
      // A link to it opens it.
      await page.goto("/overlays?record=r3");
      await expect(page.getByTestId("sheet").getByRole("heading")).toHaveText("payments-0003");
      await page.getByTestId("sheet-close").click();
      await expect(page.getByTestId("sheet")).toBeHidden();
    });

    test("Popover: opens at its trigger, takes typing, and closes on Esc with the focus back", async ({ page }) => {
      await open(page, "/overlays", look);
      const opener = page.getByTestId("open-popover");
      await opener.click();
      await expect(page.getByTestId("popover")).toBeVisible();
      await page.getByTestId("popover-input").fill("250");
      await expect(page.getByTestId("popover-value")).toHaveText("250");
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("popover")).toBeHidden();
      await expect(opener).toBeFocused();
      await opener.click();
      await page.getByTestId("popover-done").click();
      await expect(page.getByTestId("popover")).toBeHidden();
    });

    test("Command palette: the shortcut opens it, typing narrows, Enter goes, and the place is remembered", async ({ page, browserName }) => {
      await open(page, "/overlays", look);
      await page.evaluate(() => localStorage.removeItem("doze-gallery.recents"));
      // Chromium and Firefox on macOS take either modifier; the page listens for both.
      await page.keyboard.press(browserName === "webkit" ? "Meta+k" : "Control+k");
      const input = page.getByTestId("palette-input");
      await expect(input).toBeFocused();
      const list = page.getByTestId("palette-list");
      await expect(list.getByRole("option", { name: "Foundations" })).toBeVisible();
      await input.pressSequentially("stat");
      await expect(list.getByRole("option", { name: "States" })).toBeVisible();
      await expect(list.getByRole("option", { name: "Foundations" })).toHaveCount(0);
      await input.fill("zzzz");
      await expect(list).toContainText("Nothing is called “zzzz”.");
      await input.fill("Data");
      await page.keyboard.press("Enter");
      await expect(input).toBeHidden();
      await expect(page).toHaveURL(/\/data$/);
      await expect(page.locator("h1").first()).toHaveText("Data");

      // The trigger in the top bar opens it too, the place just visited is first, and Esc gives the focus back to the trigger.
      const trigger = page.getByTestId("palette-trigger");
      await trigger.click();
      await expect(list.getByRole("option").first()).toContainText("Data");
      await expect(list).toContainText("Recents");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("Escape");
      await expect(input).toBeHidden();
      await expect(trigger).toBeFocused();
    });

    test("Select: the keyboard opens it, moves, chooses and closes; Esc keeps the value", async ({ page }) => {
      await open(page, "/forms", look);
      const trigger = page.getByTestId("select");
      const value = page.getByTestId("select-value");
      await trigger.focus();
      await page.keyboard.press("Enter");
      const listbox = page.getByRole("listbox");
      await expect(listbox).toBeVisible();
      await expect(listbox.getByRole("option", { name: "us-east-1" })).toHaveAttribute("aria-selected", "true");
      // At a person's pace: Radix gives the chosen option the focus as the list finishes opening.
      await expect(listbox.getByRole("option", { name: "us-east-1" })).toBeFocused();
      await page.keyboard.press("ArrowDown");
      await expect(listbox.getByRole("option", { name: "us-west-2" })).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(listbox).toBeHidden();
      await expect(value).toHaveText("us-west-2");
      await expect(trigger).toBeFocused();
      // The mouse, and Esc.
      await trigger.click();
      await page.getByRole("option", { name: "eu-west-1" }).click();
      await expect(value).toHaveText("eu-west-1");
      await trigger.click();
      await expect(page.getByRole("listbox")).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(page.getByRole("listbox")).toBeHidden();
      await expect(value).toHaveText("eu-west-1");
    });

    test("Combobox and MultiSelect: search, choose by the keyboard, a typed pattern, and several with chips", async ({ page }) => {
      await open(page, "/forms", look);
      const one = page.getByRole("combobox", { name: "Queue", exact: true });
      await one.click();
      const search = page.getByPlaceholder("Search queues…");
      await expect(search).toBeFocused();
      await search.pressSequentially("pay");
      await expect(page.getByRole("option")).toHaveCount(1);
      await page.keyboard.press("Enter");
      await expect(search).toBeHidden();
      await expect(page.getByTestId("combobox-value")).toHaveText("payments");
      await expect(one).toHaveText(/payments/);

      const pattern = page.getByRole("combobox", { name: "Name or pattern" });
      await pattern.click();
      await page.getByPlaceholder("Search…").pressSequentially("orders-*");
      await page.getByTestId("combobox-pattern").click();
      await expect(page.getByTestId("pattern-value")).toHaveText("orders-*");
      await expect(page.getByPlaceholder("Search…")).toBeHidden();

      const many = page.getByRole("combobox", { name: "Queues", exact: true });
      await many.click();
      await page.getByRole("option", { name: /^inventory/ }).click();
      await expect(page.getByTestId("multi-value")).toHaveText("orders,payments,inventory");
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("multi-chips").getByText("inventory")).toBeVisible();
      await page.getByRole("button", { name: "Remove orders" }).click();
      await expect(page.getByTestId("multi-value")).toHaveText("payments,inventory");
      await page.getByRole("button", { name: "Clear queues" }).click();
      await expect(page.getByTestId("multi-value")).toHaveText("all");
      await expect(page.getByTestId("multi-summary")).toHaveText("All queues");
    });
  });
}

test("DropdownMenu, Tooltip and the form: a menu by the keyboard, a tooltip on focus, errors under their fields", async ({ page }) => {
  await open(page, "/overlays", { theme: "aws", mode: "light" });
  const menuOpener = page.getByTestId("open-menu");
  await menuOpener.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("menu")).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Send a message" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("menuitem", { name: "Peek" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("menu")).toBeHidden();
  await expect(page.getByTestId("menu-said")).toContainText("Peek");
  await expect(menuOpener).toBeFocused();

  await page.getByTestId("tooltip-trigger").focus();
  await expect(page.getByRole("tooltip")).toHaveText("The exact value, in words");
  await page.keyboard.press("Escape");

  await page.goto("/forms");
  await page.getByTestId("f-timeout").fill("12 hours");
  await page.getByTestId("f-submit").click();
  const form = page.getByTestId("the-form");
  await expect(form).toContainText("A queue needs a name.");
  await expect(form).toContainText("Whole seconds, from 0 to 43,200 (12 hours).");
  await expect(page.getByTestId("f-name")).toHaveAttribute("aria-invalid", "true");
  await page.getByTestId("f-name").fill("orders");
  await page.getByTestId("f-timeout").fill("45");
  await page.getByTestId("f-submit").click();
  await expect(page.getByTestId("f-done")).toContainText("Queue orders would be created, hiding a received message for 45 s.");
});
