// A dead-letter queue's redrive: the move tasks as ListMessageMoveTasks gives them, and what each state says (sqs__dlq-redrive-*).
export type Task = {
  TaskHandle?: string;
  Status: string;
  SourceArn: string;
  DestinationArn?: string;
  MaxNumberOfMessagesPerSecond?: number;
  ApproximateNumberOfMessagesMoved?: number;
  ApproximateNumberOfMessagesToMove?: number;
  FailureReason?: string;
  StartedTimestamp?: number;
};

export const RUNNING = "RUNNING";
export const tail = (arn: string | undefined): string => arn?.split(":").pop() ?? "";

/** The newest task (SQS lists them newest first). */
export const latest = (tasks: readonly Task[] | undefined): Task | null => tasks?.[0] ?? null;

/** Where a task sends: the source queues, or the named destination. */
export const destinationWords = (t: Task): string => (t.DestinationArn ? tail(t.DestinationArn) : "Source queue(s)");

/** `System optimized`, or the rate that was set. */
export const rateWords = (t: Task): string => (t.MaxNumberOfMessagesPerSecond ? `${t.MaxNumberOfMessagesPerSecond.toLocaleString("en-US")} ${t.MaxNumberOfMessagesPerSecond === 1 ? "message" : "messages"} per second` : "System optimized");

const n = (v: number): string => `${v.toLocaleString("en-US")} ${v === 1 ? "message" : "messages"}`;

/** The sentence under the dead-letter queue's name once a task has been run, or null when the queue's own sentence stands. */
export function taskSentence(t: Task | null, dlq: string, holds: number): string | null {
  if (!t) return null;
  const moved = t.ApproximateNumberOfMessagesMoved ?? 0;
  const total = t.ApproximateNumberOfMessagesToMove ?? 0;
  const to = t.DestinationArn ? tail(t.DestinationArn) : "their source queues";
  switch (t.Status) {
    case "RUNNING":
      return `${dlq} is moving ${n(Math.max(total - moved, 0) || total)} back to ${to}. Only one redrive can run on a queue at a time.`;
    case "COMPLETED":
      return holds === 0 ? `${dlq} is empty. ${moved === 1 ? "Its message" : "Its messages"} went back to ${to}${t.DestinationArn ? "" : ", the queue they were dead-lettered from"}.` : `${dlq} holds ${n(holds)}. The last redrive moved ${n(moved)} to ${to}.`;
    case "CANCELLED":
      return `${dlq} still holds ${n(holds)}. You cancelled the redrive${moved === 0 ? " before it moved any" : ` after it moved ${n(moved)}`}; start another when you are ready.`;
    case "FAILED":
      return `${dlq} still holds ${n(holds)}. The last redrive failed${t.FailureReason === "CouldNotDetermineMessageSource" ? " because SQS has no source queue to return that message to" : ""}.`;
    default:
      return null;
  }
}

/** What a finished or failed task says under the panel. */
export function statusNote(t: Task): string | null {
  if (t.Status === "FAILED" && t.FailureReason === "CouldNotDetermineMessageSource") return `Failed: ${t.FailureReason}. SQS could not tell which queue this message came from, because it was sent straight to the dead-letter queue with SendMessage. Nothing was moved. Start the redrive again and choose Redrive to custom destination, then enter the queue's ARN.`;
  if (t.Status === "FAILED") return `Failed${t.FailureReason ? `: ${t.FailureReason}` : ""}. Messages already moved stay at the destination.`;
  if (t.Status === "CANCELLED") return "CancelMessageMoveTask stopped the task. Messages it had already moved stay at the destination; the rest stay in the dead-letter queue, and a new task can be started.";
  return null;
}
