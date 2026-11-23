import { describe, expect, test } from "bun:test";
import { emptyWizard } from "./wizard-model";
import { keepWizard, takeWizard } from "./wizard-keep";

const k = (stack: string) => ({ stack, w: emptyWizard("update"), step: 3, furthest: 3, summary: undefined, previewId: "console-preview-1" });

describe("the update wizard kept while its preview is read", () => {
  test("coming back for the same stack gives it back", () => {
    keepWizard(k("larder-core"));
    expect(takeWizard("larder-core", true)?.step).toBe(3);
    expect(takeWizard("larder-core", true)?.previewId).toBe("console-preview-1");
  });
  test("another stack, or a new Update, starts fresh and drops it", () => {
    keepWizard(k("larder-core"));
    expect(takeWizard("larder-search", true)).toBeNull();
    keepWizard(k("larder-core"));
    expect(takeWizard("larder-core", false)).toBeNull();
    expect(takeWizard("larder-core", true)).toBeNull();
  });
});
