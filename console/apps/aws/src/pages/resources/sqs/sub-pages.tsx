// The pages a queue opens with `?view=`: Edit, Send and receive messages, Start DLQ redrive, and the two forms that wire the queue to
// Lambda (sqs__queue-lambda-configure) and SNS (sqs__queue-sns-subscribe).
import { lazy, Suspense, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Field, FormPage, FormSection, resourceCrumbs } from "../../../kit";
import { useOp } from "../deep/api";
import { ApiFailure } from "@doze/ui/lib/api";
import { wait, type Queue } from "./queue-model";
import { statementCount, topicHint, withTopic } from "./subscribe-model";

const Edit = lazy(() => import("./edit").then((m) => ({ default: m.SqsEdit })));
const Messages = lazy(() => import("./messages").then((m) => ({ default: m.SqsMessages })));
const Redrive = lazy(() => import("./redrive").then((m) => ({ default: m.SqsRedrive })));

export type SubProps = { d: ResourceDetail; q: Queue; back: () => void; done: (done: string, tab?: string) => void };

/** The page for `?view=`; an unknown one says so instead of showing nothing. */
export function SubPage({ view, ...p }: SubProps & { view: string }) {
  return (
    <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}>
      {view === "edit" ? <Edit {...p} /> : view === "messages" ? <Messages {...p} /> : view === "redrive" ? <Redrive {...p} /> : view === "lambda-configure" ? <ConfigureTrigger {...p} /> : view === "sns-subscribe" ? <SubscribeTopic {...p} /> : <Alert tone="warn" title="No such page">This queue has no page called {view}.</Alert>}
    </Suspense>
  );
}

const crumbs = (d: ResourceDetail, tab: string, label: string) => resourceCrumbs(d, { label, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}?tab=${tab}` });

/** A choice from another kind's list, or an alias typed in whole (an ARN of one that is not listed). */
function ListPicker({ service, kind, noun, value, onChange, by, id }: { service: string; kind: string; noun: string; value: string | null; onChange: (v: string | null) => void; by: "id" | "name"; id: string }) {
  const list = useResourceList(service, kind);
  const rows = list.data?.pages.flatMap((p) => p.rows) ?? [];
  return <Combobox id={id} options={rows.map((r) => ({ value: by === "id" ? r.id : r.name, label: r.label })).sort((a, b) => a.label.localeCompare(b.label))} value={value} onChange={onChange} placeholder={list.isPending ? `Reading ${noun}s…` : `Choose a ${noun}`} search={`Search ${noun}s, or enter an ARN…`} empty={`No ${noun} matches.`} className="w-full font-mono" pattern aria-label={`Choose a ${noun}`} />;
}

function failure(e: unknown): { code: string; message: string } | null {
  if (!e) return null;
  return e instanceof ApiFailure ? { code: e.code, message: e.message } : { code: "Error", message: (e as Error).message };
}

export function ConfigureTrigger({ d, q, back, done }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const [chose, setFn] = useState<string | null | undefined>(undefined);
  const make = useOp(t, "CreateEventSourceMapping");
  const functions = useResourceList("lambda", "functions");
  const names = (functions.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name).sort();
  // The first function is chosen until the person chooses another (or clears the choice).
  const fn = chose === undefined ? (names[0] ?? null) : chose;
  const fname = fn?.startsWith("arn:") ? fn.split(":").pop()! : fn;
  const chosen = useResourceDetail("lambda", "functions", fname ?? "", Boolean(fname) && names.includes(fname ?? ""));
  const timeout = Number(chosen.data?.fields.find((f) => f.label === "Timeout")?.value);
  const tooLong = Number.isFinite(timeout) && timeout > q.visibility;
  const refused = failure(make.error);
  const others = names.filter((n) => n !== fname).slice(0, 3);
  return (
    <FormPage
      title="Configure Lambda function trigger"
      sub={`Lambda will poll ${q.name} and invoke the function with each batch. The queue and the function must be in the same Region.`}
      crumbs={crumbs(d, "lambda", "Lambda triggers")}
      request={fn ? { service: "lambda", operation: "CreateEventSourceMapping", input: { EventSourceArn: q.arn, FunctionName: fn } } : null}
      refusal={refused}
      callNote="An event source mapping with Lambda's defaults for every setting not shown."
      callService
      banner={refused ? <Alert tone="bad" title="The trigger was not made." testId="trigger-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Save"
      pending={make.isPending}
      disabled={!fn}
      disabledWhy="Choose the function that will poll the queue."
      onCancel={back}
      onSubmit={() => make.mutate({ FunctionName: fn }, { onSuccess: () => done("trigger-created", "lambda") })}
      testId="trigger-form"
    >
      <FormSection title="Lambda trigger">
        <Field label="Lambda function" hint={others.length > 0 ? `Choose an existing function, or enter the ARN of one that isn't listed. Also here: ${others.join(", ")}.` : "Choose an existing function, or enter the ARN of one that isn't listed."} testId="field-function">
          {(id) => <div className="grid gap-2"><ListPicker id={id} service="lambda" kind="functions" noun="function" value={fn} onChange={(v) => { make.reset(); setFn(v); }} by="name" />{chosen.data && <p className="font-mono text-[12.5px] break-all text-muted-foreground" data-testid="function-arn">{chosen.data.fields.find((f) => f.label === "ARN")?.value}</p>}</div>}
        </Field>
        <Callout tone="warn" lead={tooLong ? `${fname} has a timeout of ${wait(timeout)}, longer than this queue's visibility timeout.` : "Check the visibility timeout first."} testId="visibility-check">
          The queue's visibility timeout must be at least the function's timeout, or the trigger is refused; AWS recommends six times it. {q.name} uses {wait(q.visibility)}, so a function with a longer timeout is refused here.
        </Callout>
        <Callout tone="note" testId="trigger-role-note">The function's execution role needs sqs:ReceiveMessage, sqs:DeleteMessage and sqs:GetQueueAttributes on the queue. Whoever saves this needs lambda:CreateEventSourceMapping, lambda:ListEventSourceMappings and lambda:ListFunctions.</Callout>
      </FormSection>
    </FormPage>
  );
}

export function SubscribeTopic({ d, q, back, done }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const [picked, setTopic] = useState<string | null | undefined>(undefined);
  const subscribe = useOp(t, "Subscribe");
  const settings = useOp(t, "SetQueueAttributes");
  const topics = useResourceList("sns", "topics");
  const rows = topics.data?.pages.flatMap((p) => p.rows) ?? [];
  // The topic with the most deliveries is chosen until the person chooses another (or clears the choice).
  const topic = picked === undefined ? ([...rows].sort((a, b) => Number(b.cells[1]) - Number(a.cells[1]) || a.name.localeCompare(b.name))[0]?.id ?? null) : picked;
  const mine = rows.find((r) => r.id === topic);
  const others = rows.filter((r) => r.id !== topic);
  const plan = topic ? withTopic(q.policy, q.arn, topic) : null;
  const refused = failure(subscribe.error ?? settings.error);
  const save = async () => {
    if (!topic || !plan) return;
    if (plan.added) await settings.mutateAsync({ Attributes: { Policy: plan.policy } });
    subscribe.mutate({ TopicArn: topic }, { onSuccess: () => done("subscribed", "sns") });
  };
  return (
    <FormPage
      title="Subscribe to Amazon SNS topic"
      sub={`Every message published to the topic will be delivered to ${q.name}. SQS manages the subscription and the permission that lets the topic send.`}
      crumbs={crumbs(d, "sns", "SNS subscriptions")}
      request={topic ? { service: "sns", operation: "Subscribe", input: { TopicArn: topic, Protocol: "sqs", Endpoint: q.arn } } : null}
      refusal={refused}
      callNote="Protocol sqs, with the queue's ARN as the endpoint. The access policy statement is written to the queue through SQS."
      callService
      banner={refused ? <Alert tone="bad" title="The subscription was not made." testId="subscribe-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Save"
      pending={subscribe.isPending || settings.isPending}
      disabled={!topic}
      disabledWhy="Choose the topic to subscribe to."
      onCancel={back}
      onSubmit={() => void save().catch(() => undefined)}
      testId="subscribe-form"
    >
      <FormSection title="Amazon SNS topic">
        <Field label="Specify an Amazon SNS topic available for this queue" hint={topicHint(others, mine)} testId="field-topic">
          {(id) => <div className="grid gap-2"><ListPicker id={id} service="sns" kind="topics" noun="topic" value={topic} onChange={setTopic} by="id" />{topic && <p className="font-mono text-[12.5px] break-all text-muted-foreground" data-testid="topic-arn">{topic}</p>}</div>}
        </Field>
        {plan && (
          <Callout tone="info" lead={plan.added ? `This adds a statement to the access policy of ${q.name}.` : "The access policy already allows it."} testId="policy-statement">
            {plan.added ? <>The topic may send to it; {q.name} has {statementCount(q.policy)} {statementCount(q.policy) === 1 ? "statement" : "statements"} today and would have {plan.count}.</> : <>The access policy of {q.name} already lets {mine?.name ?? "the topic"} send to it, so it is left as it is.</>} The queue owner is creating the subscription, so it is confirmed automatically.
          </Callout>
        )}
      </FormSection>
    </FormPage>
  );
}

