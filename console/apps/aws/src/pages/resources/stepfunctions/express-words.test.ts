import { describe, expect, test } from "bun:test";
import type { AslState } from "./graph";
import { expressWords, inputSkeleton, listSentence, machineSentence } from "./model";

describe("what is said of an Express machine's runs", () => {
  test("logging Off (the default) records nothing, and the page says so", () => {
    expect(expressWords("OFF")).toContain("its logging is Off, so its runs are recorded nowhere");
    expect(expressWords("OFF")).not.toContain("written to its log group");
  });

  test("logging ALL writes the runs to the log group; ERROR and FATAL only the failures", () => {
    expect(expressWords("ALL")).toBe("it keeps no execution history: its runs are written to its log group.");
    expect(expressWords("ERROR")).toContain("only the events that report a failure");
    expect(expressWords("fatal")).toContain("(logging FATAL)");
  });

  test("a page that has not read the level does not claim a log group", () => {
    expect(expressWords()).toBe("it keeps no execution history: its runs are written to a log group only while its logging is on.");
    expect(listSentence([{ name: "thumbnail-batch", express: true }], {})).toBe("thumbnail-batch is Express, so it keeps no execution history: its runs are written to a log group only while its logging is on.");
    expect(listSentence([{ name: "a", express: true }, { name: "b", express: true }], {})).toContain("a and b are Express");
  });

  test("a machine's own sentence follows its level", () => {
    expect(machineSentence("thumbnail-batch", true, undefined, undefined, "OFF")).toContain("recorded nowhere");
    expect(machineSentence("thumbnail-batch", true, undefined, undefined, "ALL")).toContain("written to its log group");
  });
});

describe("what a new execution starts with", () => {
  const def = (state: AslState) => ({ StartAt: "A", States: { A: state } });

  test("is a skeleton of the paths the first state reads", () => {
    const text = inputSkeleton(def({ Type: "Task", Parameters: { "Id.$": "$.order_id", "Who.$": "$.customer.email", "At.$": "$$.Execution.StartTime", Fixed: 1 }, ResultPath: "$.done" }));
    expect(JSON.parse(text)).toEqual({ order_id: "", customer: { email: "" } });
  });

  test("is {} when the first state reads nothing, or the definition is not there", () => {
    expect(inputSkeleton(def({ Type: "Pass", ResultPath: "$.out" }))).toBe("{}");
    expect(inputSkeleton(null)).toBe("{}");
  });

  test("reads a Choice's Variable and an InputPath", () => {
    expect(JSON.parse(inputSkeleton(def({ Type: "Choice", Choices: [{ Variable: "$.level", StringEquals: "a", Next: "A" }] })))).toEqual({ level: "" });
    expect(JSON.parse(inputSkeleton(def({ Type: "Task", InputPath: "$.order" })))).toEqual({ order: "" });
  });
});
