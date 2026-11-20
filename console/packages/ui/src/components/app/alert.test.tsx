import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ApiFailure } from "../../lib/api";
import { Alert, ErrorBlock, errorWords } from "./alert";

test("an alert says its title and body, and is announced by its tone", () => {
  const bad = renderToStaticMarkup(<Alert tone="bad" title="It failed">Why it failed.</Alert>);
  expect(bad).toContain('role="alert"');
  expect(bad).toContain('data-tone="bad"');
  expect(bad).toContain("It failed");
  expect(bad).toContain("Why it failed.");
  expect(renderToStaticMarkup(<Alert tone="warn" title="Careful" />)).toContain('role="alert"');
  expect(renderToStaticMarkup(<Alert tone="info" title="Note" />)).toContain('role="status"');
  expect(renderToStaticMarkup(<Alert tone="good" title="Done" />)).toContain('role="status"');
  // The default is a note.
  expect(renderToStaticMarkup(<Alert title="Note" />)).toContain('data-tone="info"');
});

test("an alert without a body or an action has neither box", () => {
  const html = renderToStaticMarkup(<Alert title="Only a title" />);
  expect(html.match(/<div/g)?.length).toBe(3);
});

test("an error's words are the server's, with its code and status", () => {
  expect(errorWords(new ApiFailure(403, { code: "FORBIDDEN", message: "Not from here." }))).toEqual({ message: "Not from here.", code: "FORBIDDEN · HTTP 403" });
  // No answer at all: there is no status to show.
  expect(errorWords(new ApiFailure(0, { code: "UNREACHABLE", message: "It did not answer." }))).toEqual({ message: "It did not answer.", code: "UNREACHABLE" });
  expect(errorWords(new Error("boom"))).toEqual({ message: "boom" });
  expect(errorWords(undefined)).toEqual({ message: "No reason was given." });
  expect(errorWords("text")).toEqual({ message: "No reason was given." });
});

test("an error block says what failed, why, what to do, and offers Try again", () => {
  const html = renderToStaticMarkup(<ErrorBlock what="The services could not be loaded" error={new ApiFailure(0, { code: "UNREACHABLE", message: "doze-aws did not answer." })} hint="Check that it is still running." onRetry={() => {}} />);
  expect(html).toContain('role="alert"');
  expect(html).toContain("The services could not be loaded");
  expect(html).toContain("doze-aws did not answer.");
  expect(html).toContain("Check that it is still running.");
  expect(html).toContain("UNREACHABLE");
  expect(html).toContain("Try again");
  expect(html).toContain('data-testid="error-block-retry"');
});

test("an error block with nothing to retry has no button, and one that is retrying cannot be pressed twice", () => {
  const plain = renderToStaticMarkup(<ErrorBlock what="It failed" error={new Error("why")} />);
  expect(plain).not.toContain("<button");
  expect(plain).not.toContain('data-slot="error-code"');
  const busy = renderToStaticMarkup(<ErrorBlock what="It failed" error={new Error("why")} onRetry={() => {}} retrying />);
  expect(busy).toContain("disabled");
});
