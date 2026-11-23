import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Callout } from "./callout";

test("a callout opens with its bold lead and is announced by its tone", () => {
  const warn = renderToStaticMarkup(<Callout tone="warn" lead="Before you start.">You can't filter messages.</Callout>);
  expect(warn).toContain('role="status"');
  expect(warn).toContain('data-tone="warn"');
  expect(warn).toContain("<strong");
  expect(warn).toContain("Before you start.");
  expect(warn).toContain("You can&#x27;t filter messages.");
  expect(renderToStaticMarkup(<Callout tone="bad">Refused.</Callout>)).toContain('role="alert"');
  expect(renderToStaticMarkup(<Callout tone="note">On AWS the caller needs a permission.</Callout>)).toContain('role="status"');
  // The default is an info box, and a callout with no lead has no bold.
  const plain = renderToStaticMarkup(<Callout>Just words.</Callout>);
  expect(plain).toContain('data-tone="info"');
  expect(plain).not.toContain("<strong");
});
