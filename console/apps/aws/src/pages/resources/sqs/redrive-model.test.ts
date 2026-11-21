import { expect, test } from "bun:test";
import { cancellable, destinationWords, latest, rateWords, redriveBanner, statusNote, statusTone, taskSentence, type Task } from "./redrive-model";

const task = (over: Partial<Task>): Task => ({ Status: "RUNNING", SourceArn: "arn:aws:sqs:us-east-1:1:pick-lists-dlq", ApproximateNumberOfMessagesMoved: 0, ApproximateNumberOfMessagesToMove: 1, ...over });

test("each state says what became of the messages", () => {
  expect(taskSentence(task({}), "pick-lists-dlq", 1)).toBe("pick-lists-dlq is moving 1 message back to their source queues. Only one redrive can run on a queue at a time.");
  expect(taskSentence(task({ Status: "COMPLETED", ApproximateNumberOfMessagesMoved: 1 }), "pick-lists-dlq", 0)).toBe("pick-lists-dlq is empty. Its message went back to their source queues, the queue they were dead-lettered from.");
  expect(taskSentence(task({ Status: "CANCELLED" }), "d", 1)).toBe("d still holds 1 message. You cancelled the redrive before it moved any; start another when you are ready.");
  expect(taskSentence(task({ Status: "FAILED", FailureReason: "CouldNotDetermineMessageSource" }), "d", 1)).toBe("d still holds 1 message. The last redrive failed because SQS has no source queue to return that message to.");
  expect(taskSentence(null, "d", 1)).toBeNull();
});
test("destination, rate, newest task, and the note of a failure", () => {
  expect(destinationWords(task({}))).toBe("Source queue(s)");
  expect(destinationWords(task({ DestinationArn: "arn:aws:sqs:us-east-1:1:other" }))).toBe("other");
  expect(rateWords(task({}))).toBe("System optimized");
  expect(rateWords(task({ MaxNumberOfMessagesPerSecond: 50 }))).toBe("50 messages per second");
  expect(latest([task({ Status: "FAILED" }), task({})])?.Status).toBe("FAILED");
  expect(statusNote(task({ Status: "FAILED", FailureReason: "CouldNotDetermineMessageSource" }))).toMatchObject({ lead: "Failed: CouldNotDetermineMessageSource." });
  expect(statusNote(task({ Status: "FAILED", FailureReason: "CouldNotDetermineMessageSource" }))?.text).toContain("custom destination");
  expect(statusNote(task({ Status: "CANCELLED" }))?.lead).toBe("Status CANCELLED.");
  expect(statusNote(task({ Status: "COMPLETED" }))).toBeNull();
});

test("a running task says how many have moved, by the count the task itself gives", () => {
  expect(taskSentence(task({ ApproximateNumberOfMessagesToMove: 6, ApproximateNumberOfMessagesMoved: 1 }), "d", 5)).toBe("d is moving 6 messages back to their source queues; 1 has moved so far. Only one redrive can run on a queue at a time.");
});

test("the status has a tone", () => {
  expect(["RUNNING", "COMPLETED", "FAILED", "CANCELLED", "X"].map(statusTone)).toEqual(["info", "ok", "bad", "warn", "idle"]);
});

test("the start's banner stays only while it is true; a completed task earns its own", () => {
  const started = "redrive-started";
  expect(redriveBanner(undefined, task({}), "d", [])).toBeNull();
  expect(redriveBanner(started, null, "d", [])?.title).toBe("Redrive task started.");
  expect(redriveBanner(started, task({}), "d", [])?.title).toBe("Redrive task started.");
  expect(redriveBanner(started, task({ Status: "CANCELLED" }), "d", [])).toBeNull();
  expect(redriveBanner(started, task({ Status: "FAILED" }), "d", [])).toBeNull();
  expect(redriveBanner(started, task({ Status: "COMPLETED", ApproximateNumberOfMessagesMoved: 1 }), "pick-lists-dlq", ["pick-lists"])).toEqual({ tone: "good", title: "Status COMPLETED.", returned: "1 message moved from pick-lists-dlq to pick-lists." });
});

test("a cancel is asked for only while the newest task is the one with that handle and is running", () => {
  const run = task({ TaskHandle: "h1" });
  expect(cancellable(run, "h1")).toBe(true);
  expect(cancellable(task({ TaskHandle: "h1", Status: "COMPLETED" }), "h1")).toBe(false);
  expect(cancellable(task({ TaskHandle: "h2" }), "h1")).toBe(false);
  expect(cancellable(null, "h1")).toBe(false);
  expect(cancellable(run, undefined)).toBe(false);
});
