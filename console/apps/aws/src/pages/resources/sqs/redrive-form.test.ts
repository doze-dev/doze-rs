import { expect, test } from "bun:test";
import { startInput, startProblem } from "./redrive-form";

const S = "arn:aws:sqs:us-east-1:1:pick-lists-dlq";
test("only what is chosen is sent", () => {
  expect(startInput(S, { where: "source", dest: null, rate: "system", per: "" })).toEqual({ SourceArn: S });
  expect(startInput(S, { where: "custom", dest: "arn:x", rate: "custom", per: "50" })).toEqual({ SourceArn: S, DestinationArn: "arn:x", MaxNumberOfMessagesPerSecond: 50 });
  expect(startInput(S, { where: "source", dest: "arn:x", rate: "system", per: "50" })).toEqual({ SourceArn: S });
});
test("a custom destination and a custom rate must be given", () => {
  expect(startProblem({ where: "custom", dest: null, rate: "system", per: "" })).toContain("Choose");
  expect(startProblem({ where: "source", dest: null, rate: "custom", per: "501" })).toContain("1 to 500");
  expect(startProblem({ where: "source", dest: null, rate: "custom", per: "10" })).toBeNull();
});
