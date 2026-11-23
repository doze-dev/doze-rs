// Delete data stream (kinesis__stream-delete): says what goes with the stream (its tags, its registered consumers by name) and what the
// applications that still use it will get. Kinesis refuses a stream that has consumers unless told to drop them; the console says so and does.
import { useQuery } from "@tanstack/react-query";
import { useAbout } from "../../../api/queries";
import { useDelete } from "../../../api/resources";
import { PlainConfirm, callKindOp, useTags } from "../../../kit";
import { type Consumers, STREAMS, streamArn } from "./api";
import { count, list } from "./model";

/** What the last deletion took with it, for the list the person lands on (the banner names the consumers). */
let last: { name: string; consumers: string[] } | null = null;
export const lastDeleted = (name: string) => (last?.name === name ? last : null);

export function DeleteStream({ open, onOpenChange, name, onDone }: { open: boolean; onOpenChange: (o: boolean) => void; name: string; onDone: () => void }) {
  const about = useAbout().data;
  const del = useDelete("kinesis", "streams");
  const arn = streamArn(about?.region ?? "us-east-1", about?.account ?? "000000000000", name);
  const consumers = useQuery({ queryKey: ["resources", "kindop", "kinesis", "streams", "ListStreamConsumers", arn, "delete"], queryFn: () => callKindOp<Consumers>(STREAMS, "ListStreamConsumers", { StreamARN: arn }), enabled: open, retry: false, staleTime: 0 });
  const tags = useTags({ service: "kinesis", kind: "streams", name }, open);
  const named = (consumers.data?.Consumers ?? []).map((c) => c.ConsumerName);
  const t = tags.data?.tags.length ?? 0;
  return (
    <PlainConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
      title={<>Delete <span className="font-mono">{name}</span>?</>}
      description={<>This deletes <b className="font-mono font-semibold text-bright">{name}</b> and all its shards and data.{t > 0 ? ` Its ${count(t, "tag")} ${t === 1 ? "is" : "are"} dissociated from it.` : ""} Applications that still use it will get ResourceNotFoundException.</>}
      call="DeleteStream"
      pending={del.isPending}
      error={del.error}
      onConfirm={() => del.mutate(name, { onSuccess: () => { last = { name, consumers: named }; onDone(); } })}
      testId="delete-dialog"
    >
      {named.length > 0 && (
        <p className="text-[14px] leading-[22px] text-foreground" data-testid="delete-consumers">
          <b className="font-mono font-semibold text-bright">{name}</b> has {count(named.length, "registered consumer")}, <b className="font-mono font-semibold text-bright">{list(named)}</b>. Deleting the stream drops its consumer registrations too.
        </p>
      )}
    </PlainConfirm>
  );
}
