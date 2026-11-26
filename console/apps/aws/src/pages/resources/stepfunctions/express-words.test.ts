import { describe, expect, test } from "bun:test";
import { expressWords, listSentence, machineSentence } from "./model";

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
