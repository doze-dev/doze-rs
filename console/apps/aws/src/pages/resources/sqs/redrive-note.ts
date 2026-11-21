// What a finished redrive says to the person who started it, from the newest ListMessageMoveTasks entry.

export type MoveTask = {
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

/** SQS's own words for the failure reasons it names (the SQS Developer Guide, "Dead-letter queue redrive"); any other reason is shown as it came. */
const WHY: Record<string, string> = {
  CouldNotDetermineMessageSource: "SQS could not tell which queue the message came from, because it was sent straight to the dead-letter queue. Nothing more was moved. Start the redrive again and choose another queue as the destination.",
};

/** The sentence under a failed task: its reason, what it means, and how many moved. `null` for any task that did not fail. */
export function failedNote(task: MoveTask | undefined): { title: string; body: string } | null {
  if (task?.Status !== "FAILED") return null;
  const reason = task.FailureReason ?? "no reason given";
  const moved = task.ApproximateNumberOfMessagesMoved ?? 0;
  const of = task.ApproximateNumberOfMessagesToMove;
  const count = of === undefined ? `${moved} moved` : `${moved} of ${of} moved`;
  return { title: `The last redrive failed: ${reason}`, body: `${WHY[reason] ?? "The task stopped."} (${count}.)` };
}
