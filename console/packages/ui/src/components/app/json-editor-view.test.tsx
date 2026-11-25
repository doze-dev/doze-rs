import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { JsonEditor } from "./json-editor";

const noop = () => undefined;

test("a JSON editor has no Format button beside its label unless its page asks for one", () => {
  const plain = renderToStaticMarkup(<JsonEditor value='{"a":1}' onChange={noop} label="Policy" testId="p" />);
  expect(plain).not.toContain("Format");
  expect(plain).not.toContain('data-testid="p-format"');
  const asked = renderToStaticMarkup(<JsonEditor value='{"a":1}' onChange={noop} label="Policy" testId="p" format />);
  expect(asked).toContain('data-testid="p-format"');
});

test("text that is not JSON is said once, under the box, in the parser's words", () => {
  const html = renderToStaticMarkup(<JsonEditor value="{ not json" onChange={noop} label="Policy" testId="p" />);
  expect(html.match(/Not JSON:/g)?.length).toBe(1);
  expect(html).toContain('aria-invalid="true"');
});
