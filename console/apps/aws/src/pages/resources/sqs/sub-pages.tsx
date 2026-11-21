// The pages a queue opens with `?view=`: Edit, Send and receive messages, Start DLQ redrive, and the two forms that wire the queue to
// Lambda (sqs__queue-lambda-configure) and SNS (sqs__queue-sns-subscribe).
import { lazy, Suspense, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Field, FormPage, FormSection, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import { ApiFailure } from "@doze/ui/lib/api";
import { wait, type Queue } from "./queue-model";
import { statementCount, withTopic } from "./subscribe-model";

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

const crumbs = (d: ResourceDetail, tab: string, label: string) => [{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }, { label, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}?tab=${tab}` }];

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
  const [fn, setFn] = useState<string | null>(null);
  const make = useOp(t, "CreateEventSourceMapping");
  const functions = useResourceList("lambda", "functions");
  const names = (functions.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name);
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
          {(id) => <ListPicker id={id} service="lambda" kind="functions" noun="function" value={fn} onChange={setFn} by="name" />}
        </Field>
        <Alert tone={tooLong ? "warn" : "info"} title={tooLong ? `${fname} has a timeout of ${wait(timeout)}, longer than this queue's visibility timeout.` : "Check the visibility timeout first."} testId="visibility-check">
          The queue's visibility timeout must be at least the function's timeout, or the trigger is refused; AWS recommends six times it. {q.name} uses {wait(q.visibility)}, so a function with a longer timeout is refused here.
        </Alert>
        <RuleHelp>The function's execution role needs sqs:ReceiveMessage, sqs:DeleteMessage and sqs:GetQueueAttributes on the queue. Whoever saves this needs lambda:CreateEventSourceMapping, lambda:ListEventSourceMappings and lambda:ListFunctions.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

export function SubscribeTopic({ d, q, back, done }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const [topic, setTopic] = useState<string | null>(null);
  const subscribe = useOp(t, "Subscribe");
  const settings = useOp(t, "SetQueueAttributes");
  const topics = useResourceList("sns", "topics");
  const rows = topics.data?.pages.flatMap((p) => p.rows) ?? [];
  const mine = rows.find((r) => r.id === topic);
  const others = rows.filter((r) => r.id !== topic);
  const plan = topic ? withTopic(q.policy, q.arn, topic) : null;
  const about = useAbout().data;
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
        <Field label="Specify an Amazon SNS topic available for this queue" hint={`The topics in ${about?.account ?? "this account"} and Region are listed. If yours isn't, enter its ARN.${others.length > 0 ? ` ${others.slice(0, 2).map((r) => `${r.name}${Number(r.cells[1]) > 0 ? ` already has ${Number(r.cells[1])} ${Number(r.cells[1]) === 1 ? "subscription" : "subscriptions"}` : ""}`).join("; ")}.` : ""}`} testId="field-topic">
          {(id) => <ListPicker id={id} service="sns" kind="topics" noun="topic" value={topic} onChange={setTopic} by="id" />}
        </Field>
        {plan && (
          <RuleHelp>
            {plan.added ? <>This adds a statement to the access policy of <span className="font-mono text-foreground">{q.name}</span>, so the topic may send to it; {q.name} has {statementCount(q.policy)} {statementCount(q.policy) === 1 ? "statement" : "statements"} today and would have {plan.count}.</> : <>The access policy of {q.name} already lets {mine?.name ?? "the topic"} send to it, so it is left as it is.</>} The queue owner is creating the subscription, so it is confirmed automatically.
          </RuleHelp>
        )}
      </FormSection>
    </FormPage>
  );
}

