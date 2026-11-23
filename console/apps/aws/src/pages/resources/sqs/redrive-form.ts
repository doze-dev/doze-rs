// The Start DLQ redrive form, pure: the call it makes and what keeps it from being made.
export const MAX_RATE = 500;

export type StartForm = { where: "source" | "custom"; dest: string | null; rate: "system" | "custom"; per: string };

/** StartMessageMoveTask's input: the dead-letter queue as the source, a destination only when one is chosen, a rate only when set. */
export function startInput(sourceArn: string, f: StartForm): Record<string, unknown> {
  const n = Number(f.per);
  return {
    SourceArn: sourceArn,
    ...(f.where === "custom" && f.dest ? { DestinationArn: f.dest } : {}),
    ...(f.rate === "custom" && Number.isInteger(n) && n > 0 ? { MaxNumberOfMessagesPerSecond: n } : {}),
  };
}

/** The words of what is wrong, or null. */
export function startProblem(f: StartForm): string | null {
  if (f.where === "custom" && !f.dest) return "Choose the queue the messages go to.";
  if (f.rate === "custom") {
    const n = Number(f.per);
    if (!Number.isInteger(n) || n < 1 || n > MAX_RATE) return `The rate is a whole number of messages a second, 1 to ${MAX_RATE}.`;
  }
  return null;
}
