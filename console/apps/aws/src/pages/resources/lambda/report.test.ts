import { describe, expect, test } from "bun:test";
import { reportOf, tidyPaths } from "./report";

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

describe("a trace's file paths", () => {
  const dir = "/private/tmp/doze/data/lambda/code/40e27cc5-acc0-49b9-a120-63919bc5490c/root";
  test("lose the data directory the code was unpacked under", () => {
    expect(tidyPaths(`  File "${dir}/lambda_function.py", line 8, in lambda_handler`)).toBe('  File "…/lambda_function.py", line 8, in lambda_handler');
    // As the JSON of the answer holds it: the quotes are escaped.
    expect(tidyPaths(`"  File \\"${dir}/lib/util.py\\", line 3"`)).toBe('"  File \\"…/lib/util.py\\", line 3"');
  });
  test("leave other paths and text alone", () => {
    expect(tidyPaths('File "/usr/lib/python3.13/json/__init__.py", line 1')).toBe('File "/usr/lib/python3.13/json/__init__.py", line 1');
    expect(tidyPaths("no path here")).toBe("no path here");
  });
});
