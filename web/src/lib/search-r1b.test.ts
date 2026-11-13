import { describe, expect, test } from "bun:test";
import { configSearch, createSearch, topicWriteSearch } from "./search";

describe("R1b search params keep only what is allowed", () => {
  test("Create topic's step", () => {
    expect(createSearch({ step: "review" })).toEqual({ step: "review" });
    expect(createSearch({ step: "name" })).toEqual({ step: undefined });
    expect(createSearch({ step: "<x>" })).toEqual({ step: undefined });
  });
  test("a topic's overlay and its partition", () => {
    expect(topicWriteSearch({ dialog: "produce" })).toEqual({ dialog: "produce", partition: undefined });
    expect(topicWriteSearch({ dialog: "delete-records", partition: "2" })).toEqual({ dialog: "delete-records", partition: 2 });
    expect(topicWriteSearch({ dialog: "drop", partition: "-1" })).toEqual({ dialog: undefined, partition: undefined });
    expect(topicWriteSearch({ partition: "1e9" })).toEqual({ dialog: undefined, partition: undefined });
  });
  test("the Config tab's filter, chip and dialogs", () => {
    expect(configSearch({ q: "dirty", show: "changed", edit: "retention.ms" })).toEqual({ q: "dirty", show: "changed", edit: "retention.ms", reset: undefined });
    expect(configSearch({ show: "all", q: "x".repeat(121) })).toEqual({ q: undefined, show: undefined, edit: undefined, reset: undefined });
  });
});
