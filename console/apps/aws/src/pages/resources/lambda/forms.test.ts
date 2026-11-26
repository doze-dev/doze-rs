import { expect, test } from "bun:test";
import { layerInput } from "./forms";

test("PublishLayerVersion's members are in the board's order: name, description, code, then what it is compatible with", () => {
  const input = layerInput({ name: "mail-templates", description: "HTML and text templates", license: "MIT", archs: ["x86_64"], runtimes: ["python3.13", "nodejs22.x"] }, "AAAA");
  expect(Object.keys(input)).toEqual(["LayerName", "Description", "Content", "CompatibleArchitectures", "CompatibleRuntimes", "LicenseInfo"]);
  expect(Object.keys(layerInput({ name: "l", description: "", license: "", archs: [], runtimes: [] }, ""))).toEqual(["LayerName", "Content"]);
});
