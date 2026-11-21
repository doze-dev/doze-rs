// Edit queue (sqs__queue-edit): the queue's own settings, in the same fields as Create, saving only what changed with
// SetQueueAttributes, and its tags with TagQueue and UntagQueue.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { rowsValid } from "@doze/ui/components/app/key-values";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { FormPage, TAG_RULES, diffTags, noChange, useSetTags, useTags } from "../../../kit";
import { useOp } from "../deep/api";
import { Configuration, TypeAndName } from "./form-fields";
import { attributes, formOf, problems, type QueueForm } from "./form-model";
import { AccessPolicy, DeadLetter, Encryption, RedriveAllow, TagsSection } from "./form-sections";
import type { SubProps } from "./sub-pages";

export function SqsEdit(p: SubProps) {
  const t = { service: p.d.service, kind: p.d.kind, name: p.d.name };
  const tags = useTags(t);
  if (tags.isPending) return <Skeleton className="h-96 rounded-[14px]" data-testid="edit-loading" />;
  return <EditForm {...p} tags={tags.data?.tags ?? []} />;
}

function EditForm({ d, q, back, done, tags }: SubProps & { tags: { key: string; value: string }[] }) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const about = useAbout().data;
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const dlqArn = q.dlq ? `arn:aws:sqs:${region}:${account}:${q.dlq}` : null;
  const base = useMemo(() => ({ create: false as const, queue: q, dlqArn }), [q, dlqArn]);
  const [f, setF] = useState<QueueForm>(() => formOf(q, tags, dlqArn));
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<QueueForm>) => setF((x) => ({ ...x, ...patch }));
  const save = useOp(t, "SetQueueAttributes");
  const setTags = useSetTags(t);
  const p = problems(f, { create: false });
  const changes = useMemo(() => attributes(f, base, q.arn), [f, base, q.arn]);
  const tagChange = diffTags(tags, f.tags);
  const nothing = Object.keys(changes).length === 0 && noChange(tagChange);
  const bad = Object.keys(p).length > 0 || !rowsValid(f.tags, TAG_RULES);
  const error = save.error ?? setTags.error;
  const refused = error instanceof ApiFailure ? { code: error.code, message: error.message } : error ? { code: "Error", message: (error as Error).message } : null;
  const submit = async () => {
    setTried(true);
    if (bad || nothing) return;
    try {
      if (Object.keys(changes).length > 0) await save.mutateAsync({ Attributes: changes });
      if (!noChange(tagChange)) await setTags.mutateAsync(tagChange);
      done("updated");
    } catch {
      /* the refusal is shown */
    }
  };
  return (
    <FormPage
      title={`Edit ${d.name}`}
      sub={<>{q.fifo ? "FIFO" : "Standard"}. You can edit the queue's configuration parameters, except the queue type.</>}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }]}
      mono
      request={{ service: "sqs", operation: "SetQueueAttributes", input: { QueueUrl: q.url, Attributes: changes } }}
      refusal={refused}
      callNote="Only the attribute you changed is sent. Tags you add or remove go through TagQueue and UntagQueue."
      banner={refused ? <Alert tone="bad" title="The changes were not saved." testId="edit-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Save"
      pending={save.isPending || setTags.isPending}
      disabled={nothing || (tried && bad)}
      disabledWhy={nothing ? "Nothing has changed." : Object.values(p)[0]}
      cancelLabel="Cancel"
      onCancel={back}
      onSubmit={() => void submit()}
      testId="queue-edit"
    >
      <TypeAndName f={f} set={set} p={p} show={tried} create={false} />
      <Configuration f={f} set={set} p={p} show={tried} create={false} />
      <AccessPolicy f={f} set={set} p={p} show={tried} queueArn={q.arn} basic={false} />
      <RedriveAllow f={f} set={set} p={p} show={tried} name={d.name} note={q.dlq && !d.subs.some((s) => s.title.includes("dead-letter") && s.rows.length > 0) ? `${d.name} is the source queue of ${q.dlq}, not a dead-letter queue itself, so this stays off.` : undefined} />
      <Encryption f={f} set={set} p={p} show={tried} edit />
      <DeadLetter f={f} set={set} p={p} show={tried} self={d.name} region={region} account={account} />
      <TagsSection f={f} set={set} show={tried} />
    </FormPage>
  );
}
