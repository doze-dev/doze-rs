import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../api/types/ResourceRow";
import { pickerOptions } from "./pickers";

const row = (name: string, id: string): ResourceRow => ({ name, label: name, id, cells: [], link: null });
describe("pickerOptions", () => {
  const rows = [row("pick-lists", "http://q/1/pick-lists"), row("orders", "http://q/1/orders")];
  test("valued by id, sorted by label", () => expect(pickerOptions(rows)).toEqual([{ value: "http://q/1/orders", label: "orders" }, { value: "http://q/1/pick-lists", label: "pick-lists" }]));
  test("or by name", () => expect(pickerOptions(rows, "name").map((o) => o.value)).toEqual(["orders", "pick-lists"]));
  test("nothing, nothing", () => expect(pickerOptions([])).toEqual([]));
});
