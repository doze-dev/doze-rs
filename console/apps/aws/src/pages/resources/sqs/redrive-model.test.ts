import { expect, test } from "bun:test";
import { destinationWords, latest, rateWords, statusNote, taskSentence, type Task } from "./redrive-model";

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
  expect(statusNote(task({ Status: "FAILED", FailureReason: "CouldNotDetermineMessageSource" }))).toContain("custom destination");
  expect(statusNote(task({ Status: "COMPLETED" }))).toBeNull();
});
