import { describe, expect, test } from "bun:test";
import { reportOf } from "./report";

describe("a run's REPORT line", () => {
  test("is read into its fields, the last one when there are several", () => {
    const log = "START RequestId: a\nREPORT RequestId: a\tDuration: 9 ms\nSTART RequestId: r1\nhello\nEND RequestId: r1\nREPORT RequestId: r1\tDuration: 1.23 ms\tBilled Duration: 2 ms\tMemory Size: 128 MB\tInit Duration: 80.00 ms\n";
    expect(reportOf(log)).toEqual([
      ["Request ID", "r1"],
      ["Duration", "1.23 ms"],
      ["Billed Duration", "2 ms"],
      ["Memory Size", "128 MB"],
      ["Init Duration", "80.00 ms"],
    ]);
    expect(reportOf("no report here")).toEqual([]);
  });
});
