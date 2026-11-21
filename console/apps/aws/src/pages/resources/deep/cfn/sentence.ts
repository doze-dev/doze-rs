// What a stack's page says of where it stands, and the pure reading of its events and deleted records behind it.

/** The one cause CloudFormation wrote for a failed create: the first resource event that failed. */
export type Failure = { resource: string; type: string; reason: string };

type EventLike = { LogicalResourceId?: string; ResourceType?: string; ResourceStatus?: string; ResourceStatusReason?: string; Timestamp?: number | string };

/** The resource whose creation failed first (events come newest first; a rollback then fails and deletes the rest). `null` when
 * no resource failed (the stack itself did, or nothing has). */
export function failureOf(events: EventLike[]): Failure | null {
  const failed = events.filter((e) => e.ResourceStatus === "CREATE_FAILED" && e.ResourceType !== "AWS::CloudFormation::Stack" && e.LogicalResourceId);
  const first = failed[failed.length - 1];
  if (!first?.LogicalResourceId) return null;
  return { resource: first.LogicalResourceId, type: first.ResourceType ?? "", reason: first.ResourceStatusReason ?? "" };
}

/** One sentence on where the stack stands, from its status, its reason and its times. */
export function stackSentence(name: string, status: string, why: string, created: string | null, updated: string | null): string {
  const since = updated ? `last updated ${updated}` : created ? `created ${created} and not updated since` : "";
  const head =
    status === "REVIEW_IN_PROGRESS"
      ? `${name} is waiting for you to execute its change set (${status}): it holds no resources yet.`
      : /IN_PROGRESS$/.test(status)
        ? `${name} is being worked on (${status}).`
        : /FAILED$/.test(status)
          ? `${name} is in ${status}: the last operation did not finish.`
          : status === "ROLLBACK_COMPLETE"
            ? `${name} was rolled back (${status}): its creation failed and what it had made was removed, so it holds nothing. A stack in this state can only be deleted.`
            : status === "UPDATE_ROLLBACK_COMPLETE"
              ? `${name} was rolled back (${status}): it holds what it held before the update that failed.`
              : `${name} is ${status}.`;
  return [head, since && `It was ${since}.`, why && `Reason: ${why}`].filter(Boolean).join(" ");
}

/** The sentence for a failed resource, when the stack's own reason does not say it. */
export function failureSentence(f: Failure): string {
  return `${f.resource}${f.type ? ` (${f.type})` : ""} failed to create${f.reason ? `: ${f.reason}` : "."}`;
}

type Summary = { StackName?: string; StackId?: string; DeletionTime?: number | string };

/** The newest record of a deleted stack of this name, among `ListStacks` for DELETE_COMPLETE; `null` when there is none. */
export function deletedRecord(summaries: Summary[], name: string): { id: string; deleted: string } | null {
  const mine = summaries.filter((s) => s.StackName === name && s.StackId);
  const time = (s: Summary) => Number(s.DeletionTime) || Date.parse(String(s.DeletionTime)) || 0;
  const newest = mine.sort((a, b) => time(b) - time(a))[0];
  return newest?.StackId ? { id: newest.StackId, deleted: String(newest.DeletionTime ?? "") } : null;
}
