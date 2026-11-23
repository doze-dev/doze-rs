import { expect, test } from "bun:test";
import { snippetFor } from "./app-snippets";

test("a snippet names the address it is for", () => {
  const s = snippetFor("sqs", "http://127.0.0.1:4566", "us-east-1")!;
  expect(s.cli).toBe("aws sqs create-queue --queue-name pick-lists --endpoint-url http://127.0.0.1:4566");
  expect(s.py).toContain('endpoint_url="http://127.0.0.1:4566"');
  expect(s.py).toContain("create_queue");
});
test("every service with an empty place has one, and a service without a first resource has none", () => {
  for (const s of ["sqs", "sns", "s3", "dynamodb", "kinesis", "lambda", "logs", "secretsmanager"]) expect(snippetFor(s, "http://h", "r")).not.toBeNull();
  expect(snippetFor("sts", "http://h", "r")).toBeNull();
});
