// Delete queue and Purge queue (sqs__queue-delete, queue-purge): typed confirms that say what goes, what stays and what feeds the queue.
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import type { Feeder } from "./feeders";
import { feederWords } from "./feeders";
import { messages, type Queue } from "./queue-model";

const total = (q: Queue): number => q.available + q.inFlight + q.delayed;

/** The note on what the rules that feed the queue will do once it is gone, or has been emptied. */
function feeding(fs: readonly Feeder[], after: "gone" | "emptied", name: string): string | null {
  if (fs.length === 0) return null;
  return after === "gone" ? `${feederWords(fs)} ${fs.length === 1 ? "targets" : "target"} this queue and will have nowhere to deliver.` : `${feederWords(fs)} still ${fs.length === 1 ? "delivers" : "deliver"} to ${name}, so new messages will start filling the queue again at once.`;
}
export { feeding };

export function DeleteQueue({ open, onOpenChange, q, feeders, dlqHolds, pending, error, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; q: Queue; feeders: readonly Feeder[]; dlqHolds: number | null; pending: boolean; error: unknown; onConfirm: () => void }) {
  const n = total(q);
  const failed = error ? errorWords(error) : null;
  return (
    <TypedConfirm open={open} onOpenChange={onOpenChange} title="Delete queue" words="delete" prompt={<>Type <span className="font-mono">delete</span> to confirm</>} verb="Delete" pending={pending} onConfirm={onConfirm} testId="delete-dialog"
      description={<>Delete <b className="font-mono font-semibold text-bright">{q.name}</b>{n > 0 ? <> and its {messages(n)}</> : null}? You can delete a queue that isn't empty.</>}
      refusal={failed && <Alert tone="bad" title={`DeleteQueue was refused${failed.code ? `: ${failed.code}` : ""}`} testId="delete-error">{failed.message}</Alert>}>
      <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground" data-testid="delete-words">
        Deleting removes the queue and its messages at once. AWS then refuses to create a queue of the same name for 60 seconds (QueueDeletedRecently); doze-aws does the same unless started with <span className="font-mono">--sqs-skip-60s-waits</span>.
        {feeding(feeders, "gone", q.name) && ` ${feeding(feeders, "gone", q.name)}`}
        {q.dlq && dlqHolds !== null ? ` ${q.dlq} keeps its ${messages(dlqHolds)}.` : ""}
      </p>
    </TypedConfirm>
  );
}

export function PurgeQueue({ open, onOpenChange, q, pending, error, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; q: Queue; pending: boolean; error: unknown; onConfirm: () => void }) {
  const failed = error ? errorWords(error) : null;
  const parts = [q.available > 0 && `the ${q.available} waiting`, q.inFlight > 0 && `the ${q.inFlight} in flight`, q.delayed > 0 && `the ${q.delayed} delayed`].filter(Boolean) as string[];
  return (
    <TypedConfirm open={open} onOpenChange={onOpenChange} title="Purge queue" words="purge" prompt={<>Type <span className="font-mono">purge</span> to confirm</>} verb="Purge" pending={pending} onConfirm={onConfirm} testId="purge-dialog"
      description={<>Purging <b className="font-mono font-semibold text-bright">{q.name}</b> deletes all {total(q)} of its messages{parts.length > 0 ? `, ${parts.join(" and ")}` : ""}. You can't get them back.</>}
      refusal={failed && <Alert tone="bad" title={`PurgeQueue was refused${failed.code ? `: ${failed.code}` : ""}`} testId="purge-error">{failed.message}</Alert>}>
      <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
        Purge deletes every message at once, in flight included. AWS allows one purge a queue every 60 seconds and refuses a second inside them (PurgeQueueInProgress); doze-aws does the same unless started with <span className="font-mono">--sqs-skip-60s-waits</span>. The queue and its settings stay{q.dlq ? `; ${q.dlq} is not touched` : ""}.
      </p>
    </TypedConfirm>
  );
}
