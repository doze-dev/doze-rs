// Edit topic (sns__topic-edit): the topic's settings in the same fields as Create, saved one SetTopicAttributes call for each setting that changed,
// and its tags with TagResource and UntagResource. The type and the name cannot change.
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { rowsValid } from "@doze/ui/components/app/key-values";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { diffTags, FormPage, noChange, type PreviewRequest, resourceCrumbs, TAG_RULES, useSetTags, useTags } from "../../../kit";
import { useOp } from "../deep/api";
import { Rich } from "./parts";
import { b } from "./sentences";
import { changePhrases, changes, formOfTopic, problems, type TopicForm } from "./topic-form-model";
import { Logging, RetryPolicy, TagsSection, Tracing } from "./topic-form-delivery";
import { AccessPolicy, Details, Encryption, FifoOptions, MaxSize } from "./topic-form-fields";
import type { TopicProps } from "./topic-props";
import { archiveOf, daysWords } from "./topic-model";
import { count, list } from "./words";
import { Field, FormSection, LocalStatus } from "../../../kit";
import { Input } from "@doze/ui/components/ui/input";

export function TopicEdit(p: TopicProps) {
  const tags = useTags({ service: p.d.service, kind: p.d.kind, name: p.d.name });
  if (tags.isPending) return <Skeleton className="h-96 rounded-[var(--radius-card)]" data-testid="edit-loading" />;
  return <EditForm {...p} tags={tags.data?.tags ?? []} />;
}

function EditForm({ d, t, subs, back, done, tags }: TopicProps & { tags: { key: string; value: string }[] }) {
  const target = { service: d.service, kind: d.kind, name: d.name };
  const about = useAbout().data;
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const [f, setF] = useState<TopicForm>(() => formOfTopic(t, tags, region, account));
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<TopicForm>) => setF((x) => ({ ...x, ...patch }));
  const save = useOp(target, "SetTopicAttributes", { keep: true });
  const qc = useQueryClient();
  const setTags = useSetTags(target);
  const pr = problems(f, false);
  const changed = useMemo(() => changes(f, t, region, account), [f, t, region, account]);
  const tagChange = diffTags(tags, f.tags);
  const nothing = changed.length === 0 && noChange(tagChange);
  const bad = Object.keys(pr).length > 0 || !rowsValid(f.tags, TAG_RULES);
  const error = save.error ?? setTags.error;
  const refused = error instanceof ApiFailure ? { code: error.code, message: error.message } : error ? { code: "Error", message: (error as Error).message } : null;
  const requests: PreviewRequest[] = [
    ...changed.map((c) => ({ service: "sns", operation: "SetTopicAttributes", input: { TopicArn: t.arn, AttributeName: c.name, AttributeValue: c.value } })),
    ...(tagChange.set.length > 0 ? [{ service: "sns", operation: "TagResource", input: { ResourceArn: t.arn, Tags: tagChange.set.map((x) => ({ Key: x.key, Value: x.value })) } }] : []),
    ...(tagChange.remove.length > 0 ? [{ service: "sns", operation: "UntagResource", input: { ResourceArn: t.arn, TagKeys: tagChange.remove } }] : []),
  ];
  const submit = async () => {
    setTried(true);
    if (bad || nothing) return;
    try {
      for (const c of changed) await save.mutateAsync({ AttributeName: c.name, AttributeValue: c.value });
      if (!noChange(tagChange)) await setTags.mutateAsync(tagChange);
      await qc.invalidateQueries({ queryKey: ["resources", "one", d.service, d.kind, d.name] });
      void qc.invalidateQueries({ queryKey: ["resources", "list", d.service, d.kind] });
      done("updated");
    } catch {
      /* the refusal is shown */
    }
  };
  const had = new Set(tags.map((x) => x.key));
  const tagPhrases = [...tagChange.set.map((x) => `the tag ${x.key} is ${had.has(x.key) ? "changed" : "added"}`), ...tagChange.remove.map((k) => `the tag ${k} is removed`)];
  const phrases = [...changePhrases(changed), ...tagPhrases];
  const own = { total: subs.length, sqsOrLambda: subs.filter((s) => s.protocol === "sqs" || s.protocol === "lambda").length };
  return (
    <FormPage
      callFormat="outline"
      title={`Edit ${d.name}`}
      sub="The type and the name cannot change. Every other setting can."
      crumbs={resourceCrumbs(d)}
      mono
      request={requests[0] ?? null}
      requests={requests}
      refusal={refused}
      callNote={<>SetTopicAttributes changes one attribute per call, so Save changes sends one call for each setting you changed. Removed tags go in one UntagResource call.</>}
      banner={
        <div className="grid gap-4">
          {refused ? <Alert tone="bad" title="The changes were not saved." testId="edit-refused">{refused.code}: {refused.message}</Alert> : null}
          <Rich testId="edit-sentence" parts={nothing ? ["Nothing has changed yet."] : ["You changed ", b(count(phrases.length, "setting")), `: ${list(phrases)}. Nothing else will be sent.`]} />
        </div>
      }
      submitLabel="Save changes"
      pending={save.isPending || setTags.isPending}
      disabled={nothing || (tried && bad)}
      disabledWhy={nothing ? "Nothing has changed." : Object.values(pr)[0]}
      onCancel={back}
      onSubmit={() => void submit()}
      testId="topic-edit"
    >
      <Details f={f} set={set} p={pr} show={tried} create={false} mono />
      {t.fifo && <FifoOptions f={f} set={set} />}
      <MaxSize f={f} set={set} p={pr} show={tried} collapsed={false} own={own} />
      <Encryption f={f} set={set} p={pr} show={tried} collapsed={false} />
      <AccessPolicy f={f} set={set} p={pr} show={tried} name={d.name} region={region} account={account} collapsed={false} />
      {!t.fifo && <RetryPolicy f={f} set={set} p={pr} show={tried} collapsed={false} note={subs.some((s) => s.protocol === "http" || s.protocol === "https") ? undefined : `${d.name} has no HTTP/S subscriptions, so this policy applies to nothing yet.`} />}
      <Logging f={f} set={set} p={pr} show={tried} collapsed={false} account={account} />
      {!t.fifo && <Tracing f={f} set={set} collapsed={false} />}
      {t.fifo && <Archive f={f} set={set} p={pr} show={tried} current={archiveOf(t.archive).days} />}
      <TagsSection f={f} set={set} show={tried} collapsed={false} />
    </FormPage>
  );
}

/** A FIFO topic's archive policy: how many days it keeps a copy of every message (1 to 365), or none. */
function Archive({ f, set, p, show, current }: { f: TopicForm; set: (patch: Partial<TopicForm>) => void; p: { archive?: string }; show: boolean; current: number | null }) {
  return (
    <FormSection title="Archive policy" collapsible defaultOpen={current !== null} summary={f.archive.trim() ? daysWords(Number(f.archive)) : "No archive policy"} description="Message archiving keeps a single copy of all messages published to the topic, for a minimum of one day to a maximum of 365 days." testId="section-archive">
      <Field label="Message retention period" optional error={show ? p.archive : undefined} help={<>1 to 365 days; leave empty for no archive policy. Attribute: <span className="font-mono">ArchivePolicy</span> (<span className="font-mono">MessageRetentionPeriod</span>).</>} testId="field-archive">
        {(id) => <div className="flex items-center gap-2"><Input id={id} value={f.archive} onChange={(e) => set({ archive: e.target.value })} inputMode="numeric" placeholder="None" className="w-32 bg-card font-mono" data-testid="topic-archive" /><span className="text-[13.5px] text-muted-foreground">days</span></div>}
      </Field>
      <LocalStatus state="stored" reason="Nothing is archived and nothing can be replayed; the policy round-trips for templates and apps that read it." />
    </FormSection>
  );
}
