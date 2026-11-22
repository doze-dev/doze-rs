import { describe, expect, test } from "bun:test";
import { shownName } from "./places";

describe("a place in the rail", () => {
  test("is named by the end of its ARN when its address is one (an execution)", () => {
    expect(shownName("arn:aws:states:us-east-1:111122223333:execution:order-fulfilment:ord-20261108-0417")).toBe("ord-20261108-0417");
  });

  test("keeps any other name as it is", () => {
    expect(shownName("pick-lists")).toBe("pick-lists");
    expect(shownName("/larder/prod/db/password")).toBe("/larder/prod/db/password");
  });
});
