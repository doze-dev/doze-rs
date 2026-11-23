import { expect, test } from "bun:test";
import { type FunctionRow, listText, usedBy } from "./list-model";

const fn = (name: string, layers: string[]): FunctionRow => ({ name, description: "", pkg: "Zip", runtime: "python3.13", runtimeText: "Python 3.13", modified: null, layers, role: "" });

test("a layer is used by the functions that name any of its versions, and not by one whose name only starts the same", () => {
  const fns = [fn("a", ["arn:aws:lambda:us-east-1:1:layer:larder-shared:2"]), fn("b", []), fn("c", ["arn:aws:lambda:us-east-1:1:layer:larder-shared-extra:1"]), fn("d", ["arn:aws:lambda:us-east-1:1:layer:larder-shared:1"])];
  expect(usedBy("larder-shared", fns)).toEqual(["a", "d"]);
  expect(usedBy("nope", fns)).toEqual([]);
});

test("a list of plain values reads as words, anything else stays as it is", () => {
  expect(listText('["python3.13","nodejs22.x"]')).toBe("python3.13, nodejs22.x");
  expect(listText("[]")).toBe("");
  expect(listText('[{"a":1}]')).toBe('[{"a":1}]');
  expect(listText("x86_64")).toBe("x86_64");
});
