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
      return `${dlq} is moving ${n(total)} back to ${to}${moved > 0 ? `; ${moved.toLocaleString("en-US")} ${moved === 1 ? "has" : "have"} moved so far` : ""}. Only one redrive can run on a queue at a time.`;
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

/** What a finished or failed task says in the panel: the bold lead (`Failed: CouldNotDetermineMessageSource.`) and the sentence after it. */
export function statusNote(t: Task): { lead: string; text: string } | null {
  if (t.Status === "FAILED" && t.FailureReason === "CouldNotDetermineMessageSource") return { lead: `Failed: ${t.FailureReason}.`, text: "SQS could not tell which queue this message came from, because it was sent straight to the dead-letter queue with SendMessage. Nothing was moved. Start the redrive again and choose Redrive to custom destination, then enter the queue's ARN." };
  if (t.Status === "FAILED") return { lead: t.FailureReason ? `Failed: ${t.FailureReason}.` : "Status FAILED.", text: "Messages already moved stay at the destination." };
  if (t.Status === "CANCELLED") return { lead: "Status CANCELLED.", text: "CancelMessageMoveTask stopped the task. Messages it had already moved stay at the destination; the rest stay in the dead-letter queue, and a new task can be started." };
  return null;
}

/** A status as a state word's tone: RUNNING blue, COMPLETED green, FAILED red, CANCELLED and CANCELLING amber; none is idle. */
export function statusTone(status: string | undefined): "info" | "ok" | "bad" | "warn" | "idle" {
  switch (status) {
    case "RUNNING": return "info";
    case "COMPLETED": return "ok";
    case "FAILED": return "bad";
    case "CANCELLED":
    case "CANCELLING": return "warn";
    default: return "idle";
  }
}

export type Banner = { tone: "good"; title: string; returned: string };

/** The banner a redrive earns on the page it started from, and only while it is still true: the start's while the task RUNS, the
 * finished one's once it COMPLETED (the board's `Status COMPLETED. 1 message moved from a to b.`). A cancelled or failed task has
 * its own note in the status panel, so the start's banner must not stay over it. Null when there is none. */
export function redriveBanner(done: string | undefined, t: Task | null, dlq: string, sources: readonly string[]): Banner | null {
  if (done !== "redrive-started") return null;
  if (!t || t.Status === RUNNING) return { tone: "good", title: "Redrive task started.", returned: "StartMessageMoveTask returned a task handle; it is listed while the task is RUNNING, and CancelMessageMoveTask takes it." };
  if (t.Status !== "COMPLETED") return null;
  return { tone: "good", title: "Status COMPLETED.", returned: `${n(t.ApproximateNumberOfMessagesMoved ?? 0)} moved from ${dlq} to ${t.DestinationArn ? tail(t.DestinationArn) : sources.length > 0 ? sources.join(", ") : "their source queues"}.` };
}

/** What a queue's RedriveAllowPolicy lets, in words (`Not set` when there is none). */
export function allowWords(allow: string | null): string {
  const a = allow ? (JSON.parse(allow) as { redrivePermission?: string; sourceQueueArns?: string[] }) : null;
  if (a === null) return "Not set (any source queue may use it)";
  if (a.redrivePermission === "denyAll") return "Deny all: no source queue may use it";
  if (a.redrivePermission === "byQueue") return `By queue: ${(a.sourceQueueArns ?? []).map((x) => x.split(":").pop()).join(", ")}`;
  return "Allow all source queues";
}
