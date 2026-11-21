import { describe, expect, test } from "bun:test";
import { failedNote } from "./redrive-note";

describe("a failed redrive", () => {
  test("names its reason and what moved", () => {
    const note = failedNote({ Status: "FAILED", SourceArn: "arn", FailureReason: "CouldNotDetermineMessageSource", ApproximateNumberOfMessagesMoved: 0, ApproximateNumberOfMessagesToMove: 1 });
    expect(note?.title).toBe("The last redrive failed: CouldNotDetermineMessageSource");
    expect(note?.body).toContain("sent straight to the dead-letter queue");
    expect(note?.body).toContain("0 of 1 moved");
  });
  test("a reason it does not know is shown as it came", () => {
    expect(failedNote({ Status: "FAILED", SourceArn: "arn", FailureReason: "Other" })?.title).toContain("Other");
  });
  test("no other task has a note", () => {
    expect(failedNote({ Status: "COMPLETED", SourceArn: "arn" })).toBeNull();
    expect(failedNote(undefined)).toBeNull();
  });
});
