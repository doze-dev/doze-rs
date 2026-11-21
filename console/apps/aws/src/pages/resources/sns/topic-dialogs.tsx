// Delete topic (sns__topic-delete): a typed confirm that says what goes with the topic (its subscriptions, by name) and what stays.
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import { useResourceDetail } from "../../../api/resources";
import { list } from "./words";
import { topicSubRow } from "./sub-model";
import { endpointName } from "./sub-model";

/** What must be typed, as the board asks. */
export const DELETE_WORDS = "delete me";

export function DeleteTopic({ open, onOpenChange, name, arn, pending, error, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; name: string; arn: string; pending: boolean; error: unknown; onConfirm: () => void }) {
  const d = useResourceDetail("sns", "topics", name, open);
  const rows = (d.data?.subs.find((s) => s.title === "Subscriptions")?.rows ?? []).map((r) => topicSubRow(r, arn));
  const confirmed = rows.filter((r) => !r.pending);
  const waiting = rows.filter((r) => r.pending);
  const failed = error ? errorWords(error) : null;
  const names = confirmed.map((r) => endpointName(r.protocol, r.endpoint));
  const kind = confirmed.length > 0 && confirmed.every((r) => r.protocol === "sqs") ? "queues" : confirmed.length > 0 && confirmed.every((r) => r.protocol === "lambda") ? "functions" : "endpoints";
  const mine = !d.data ? "Reading its subscriptions…" : rows.length === 0 ? <>{name} has no subscriptions.</> : <><b className="font-mono font-semibold text-bright">{name}</b> has {confirmed.length} confirmed {confirmed.length === 1 ? "one" : "ones"}{names.length > 0 ? <>: {list(names)}</> : null}{waiting.length > 0 ? <>, and {waiting.length} waiting to be confirmed</> : null}. The {kind === "endpoints" ? "endpoints" : kind} stay, and the subscriptions are deleted with the topic, at once.</>;
  return (
    <TypedConfirm open={open} onOpenChange={onOpenChange} title="Delete topic" words={DELETE_WORDS} prompt={<>Enter <span className="font-mono">{DELETE_WORDS}</span></>} verb="Delete" pending={pending} onConfirm={onConfirm} testId="delete-dialog"
      description={<>You are deleting <b className="font-mono font-semibold text-bright">{name}</b>. Deleting a topic deletes all its subscriptions; {mine}</>}
      refusal={failed && <Alert tone="bad" title={`DeleteTopic was refused${failed.code ? `: ${failed.code}` : ""}`} testId="delete-error">{failed.message}</Alert>}>
      <p className="text-[14px] leading-[22px] text-foreground" data-testid="delete-words">
        If a publisher attempts to publish to the deleted topic, it receives an error that the topic doesn't exist. Deleting a topic might prevent some messages previously sent to it from being delivered. <b className="font-semibold text-bright">Deleted topics cannot be recovered.</b>
      </p>
    </TypedConfirm>
  );
}
