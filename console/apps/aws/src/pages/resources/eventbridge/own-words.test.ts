import { describe, expect, test } from "bun:test";
import { ownBanner } from "./own-words";

describe("what an archive, a replay and an API destination say when they open from a create", () => {
  test("each says what was made and what the call returned", () => {
    expect(ownBanner("archives", "orders-7d", "created", false)?.title).toBe("Archive orders-7d created.");
    expect(ownBanner("replays", "r1", "created", false)?.returned).toContain("Starting");
    expect(ownBanner("api-destinations", "slack", "created", false)?.title).toBe("API destination slack created.");
  });

  test("says nothing when nothing was just done, and says a replay was cancelled", () => {
    expect(ownBanner("archives", "a", undefined, false)).toBeNull();
    expect(ownBanner("replays", "r1", undefined, true)?.title).toBe("Replay r1 cancelled.");
  });
});
