// Add trigger and Edit trigger (lambda__trigger-add, trigger-edit): an SQS event source mapping, made and changed with
// CreateEventSourceMapping and UpdateEventSourceMapping; and Add destination (lambda__destination-add), where a function's
// asynchronous invocations report to a queue, a topic or another function.
import { useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { useResourceDetail } from "../../../api/resources";
import { Field, FormSection, FunctionPicker, QueuePicker, TopicPicker } from "../../../kit";
import { on, useOp, useOpRead } from "../deep/api";
import { queueOf } from "../sqs/queue-model";
import { batchMost, type Destination, destinationInput, destinationProblems, mappingCreate, mappingUpdate, triggerProblems, visibilityFor } from "./forms";
import { FnFormPage, type FormProps, Muted, NotHere } from "./form-bits";
import { notFound } from "./hooks";

const arnOf = (where: FormProps["where"], name: string | null) => (name ? `arn:aws:sqs:${where.region}:${where.account}:${name}` : null);

/** The queue's own words: its type, and the topic that delivers to it when one does. */
function QueueHint({ name }: { name: string | null }) {
  const q = useResourceDetail("sqs", "queues", name ?? "", Boolean(name));
  if (!name || !q.data) return null;
  const topics = (q.data.wired ?? []).filter((w) => w.to.service === "sns").map((w) => w.to.name);
  const fifo = queueOf(q.data).fifo;
  return <span data-testid="queue-hint"><span className="font-mono text-foreground">{name}</span> is a {fifo ? "FIFO" : "standard"} queue{topics.length ? <> that the SNS topic {topics.join(", ")} delivers to</> : null}.</span>;
}

const EXTRA = "Batch window, partial batch responses, maximum concurrency and filter criteria";

/** The settings of an SQS mapping beyond its queue, enabled state and batch size: doze-aws applies each. */
function More({ window, setWindow, partial, setPartial, concurrency, setConcurrency, filter, setFilter, problems }: { window: string; setWindow: (v: string) => void; partial: boolean; setPartial: (v: boolean) => void; concurrency: string; setConcurrency: (v: string) => void; filter: string; setFilter: (v: string) => void; problems: Record<string, string> }) {
  return (
    <FormSection title={EXTRA} collapsible defaultOpen={false} description={<>Optional. doze-aws applies all four to an SQS mapping; the settings only the cloud has (provisioned pollers) are not here.</>} testId="section-more">
      <Field label="Batch window" error={problems.window} help={<>How long Lambda gathers records before it invokes the function: <b>0 to 300 seconds</b>, default <b>0</b>.</>} testId="field-window">
        {(id) => <Input id={id} value={window} onChange={(e) => setWindow(e.target.value)} inputMode="numeric" className="max-w-[12rem] font-mono" data-testid="map-window" />}
      </Field>
      <div className="flex items-start gap-3">
        <Switch id="map-partial" checked={partial} onCheckedChange={setPartial} data-testid="map-partial" />
        <Label htmlFor="map-partial" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Report batch item failures</span><span className="text-[13px] text-muted-foreground">The function answers with the messages that failed; only those go back to the queue.</span></Label>
      </div>
      <Field label="Maximum concurrency" error={problems.concurrency} help={<>The most batches that run at once: <b>2 to 1,000</b>. Leave it empty for no limit.</>} testId="field-concurrency">
        {(id) => <Input id={id} value={concurrency} onChange={(e) => setConcurrency(e.target.value)} inputMode="numeric" className="max-w-[12rem] font-mono" data-testid="map-concurrency" />}
      </Field>
      <Field label="Filter criteria" error={problems.filter} help={<>A JSON list of filters, as <span className="font-mono">{`[{"Pattern":"{\\"body\\":{\\"kind\\":[\\"invoice\\"]}}"}]`}</span>. Messages no filter matches are dropped. Leave it empty for no filter.</>} testId="field-filter">
        {(id) => <Textarea id={id} value={filter} onChange={(e) => setFilter(e.target.value)} rows={3} spellCheck={false} className="font-mono text-[13px]" data-testid="map-filter" />}
      </Field>
    </FormSection>
  );
}

function extras(window: string, partial: boolean, concurrency: string, filter: string): { input: Record<string, unknown>; problems: Record<string, string> } {
  const input: Record<string, unknown> = {};
  const problems: Record<string, string> = {};
  if (window.trim() !== "") {
    const n = Number(window);
    if (!Number.isInteger(n) || n < 0 || n > 300) problems.window = "A batch window is a whole number of seconds, 0 to 300.";
    else input.MaximumBatchingWindowInSeconds = n;
  }
  if (partial) input.FunctionResponseTypes = ["ReportBatchItemFailures"];
  if (concurrency.trim() !== "") {
    const n = Number(concurrency);
    if (!Number.isInteger(n) || n < 2 || n > 1000) problems.concurrency = "Maximum concurrency is a whole number from 2 to 1,000.";
    else input.ScalingConfig = { MaximumConcurrency: n };
  }
  if (filter.trim() !== "") {
    try {
      const v: unknown = JSON.parse(filter);
      if (!Array.isArray(v)) problems.filter = "Filter criteria are a JSON list of filters.";
      else input.FilterCriteria = { Filters: v };
    } catch {
      problems.filter = "Filter criteria are not valid JSON.";
    }
  }
  return { input, problems };
}

export function TriggerAdd(p: FormProps) {
  const { d, f, where } = p;
  const [queue, setQueue] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [batch, setBatch] = useState("10");
  const [win, setWin] = useState("");
  const [partial, setPartial] = useState(false);
  const [concurrency, setConcurrency] = useState("");
  const [filter, setFilter] = useState("");
  const [tried, setTried] = useState(false);
  const make = useOp(on.fn(d.name), "CreateEventSourceMapping");
  const arn = arnOf(where, queue);
  const more = extras(win, partial, concurrency, filter);
  const problems = { ...triggerProblems({ queue: arn, enabled, batch }, true), ...more.problems };
  const input = { ...mappingCreate(d.name, { queue: arn, enabled, batch }), ...more.input };
  const detail = useResourceDetail("sqs", "queues", queue ?? "", Boolean(queue));
  const visibility = detail.data ? queueOf(detail.data).visibility : null;
  const short = visibility !== null && f.timeout > visibility;
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    make.mutate({ ...input }, { onSuccess: () => p.go({ tab: "configuration", p: "triggers", done: "trigger-added" }) });
  };
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "triggers" }} title="Add trigger" label="Add trigger" sub={<><b className="font-medium text-foreground">Let {d.name} read from a queue.</b> Lambda polls the queue and invokes the function with batches of messages.</>}
      request={queue ? { service: "lambda", operation: "CreateEventSourceMapping", input } : null} refusal={make.error} callNote="Settings you do not send keep their defaults."
      submitLabel="Add" pending={make.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]} onSubmit={submit} testId="trigger-add-form">
      <FormSection title="Trigger configuration" testId="section-trigger">
        <Field label="Trigger type" help="This form creates a Lambda event source mapping. Triggers for S3, SNS and API Gateway are stored by those services and are not drawn here." testId="field-type">
          {(id) => (
            <Select value="sqs" onValueChange={() => undefined}>
              <SelectTrigger id={id} className="w-full max-w-[22rem] bg-card" data-testid="trigger-type"><SelectValue /></SelectTrigger>
              <SelectContent><SelectItem value="sqs">SQS</SelectItem></SelectContent>
            </Select>
          )}
        </Field>
        <Field label="SQS queue" error={tried ? problems.queue : undefined} help={<>The queue and the function must be in the same Region. <Muted><QueueHint name={queue} /></Muted></>} testId="field-queue">
          {(id) => <QueuePicker id={id} value={queue} onChange={setQueue} by="name" className="w-full max-w-[22rem] font-mono" />}
        </Field>
        <div className="flex items-start gap-3">
          <Switch id="trigger-enabled" checked={enabled} onCheckedChange={setEnabled} className="mt-1" data-testid="trigger-enabled" />
          <Label htmlFor="trigger-enabled" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Enable trigger</span><span className="text-[13px] text-muted-foreground">Selected by default. When cleared, Lambda pauses polling and invocation.</span></Label>
        </div>
        <Field label="Batch size" error={tried ? problems.batch : undefined} help={<>The maximum number of records to send to the function in each batch: up to <b>10,000</b> for a standard queue and <b>10</b> for a FIFO queue, default <b>10</b>.</>} testId="field-batch">
          {(id) => <Input id={id} value={batch} onChange={(e) => setBatch(e.target.value)} inputMode="numeric" className="max-w-[16rem] font-mono" data-testid="trigger-batch" />}
        </Field>
      </FormSection>
      <More window={win} setWindow={setWin} partial={partial} setPartial={setPartial} concurrency={concurrency} setConcurrency={setConcurrency} filter={filter} setFilter={setFilter} problems={tried ? more.problems : {}} />
      <NotHere title="Provisioned Mode" optional why="Not available in doze-aws: it allocates dedicated polling resources, which only exist in the cloud." testId="provisioned-off" />
      <Callout tone="warn" testId="visibility-note">
        Set the queue&apos;s visibility timeout to at least <b className="font-semibold text-bright">six times</b> the function&apos;s timeout ({visibilityFor(f.timeout)} seconds for the {f.timeout} second timeout of {d.name}). Lambda also refuses the mapping when the function&apos;s timeout is longer than the queue&apos;s visibility timeout.{visibility !== null && visibility < visibilityFor(f.timeout) ? <> {queue} hides a message for {visibility} {visibility === 1 ? "second" : "seconds"}{short ? ", which is shorter than the timeout: Lambda refuses the mapping." : `, less than the ${visibilityFor(f.timeout)} seconds that six times the timeout asks.`}</> : null} On AWS the execution role also needs <span className="font-mono">AWSLambdaSQSQueueExecutionRole</span>; doze-aws does not enforce it.
      </Callout>
    </FnFormPage>
  );
}

export function TriggerEdit(p: FormProps) {
  const { d } = p;
  const mapping = p.triggers.find((t) => t.how === "mapping" && t.id === p.sel);
  const [enabled, setEnabled] = useState(mapping?.enabled ?? true);
  const [batch, setBatch] = useState(String(mapping?.batch ?? 10));
  const [tried, setTried] = useState(false);
  const save = useOp(on.fn(d.name), "UpdateEventSourceMapping");
  const detail = useResourceDetail("sqs", "queues", mapping?.name ?? "", Boolean(mapping));
  const holds = detail.data ? queueOf(detail.data) : null;
  const problems = triggerProblems({ queue: mapping?.source ?? null, enabled, batch }, false);
  const from = { enabled: mapping?.enabled ?? true, batch: mapping?.batch ?? 10 };
  const input = mappingUpdate(p.sel ?? "", from, { queue: null, enabled, batch });
  const nothing = Object.keys(input).length === 1;
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0 || nothing) return;
    save.mutate({ ...input }, { onSuccess: () => p.go({ tab: "configuration", p: "triggers", done: "trigger-saved" }) });
  };
  if (!mapping) return <Callout tone="warn" testId="trigger-missing">There is no event source mapping {p.sel} on {d.name}: it may have been deleted. Go back to the Triggers section and choose one.</Callout>;
  return (
    <FnFormPage p={{ ...p, tab: "configuration", p: "triggers" }} title="Edit trigger" label="Edit trigger" sub={<><b className="font-medium text-foreground">{mapping.name}.</b> The event source mapping <span className="font-mono">{mapping.id}</span> is {mapping.enabled ? "Enabled" : "Disabled"}.</>}
      request={{ service: "lambda", operation: "UpdateEventSourceMapping", input }} refusal={save.error} callNote="The mapping is found by its UUID, which is the only handle it has."
      submitLabel="Save" pending={save.isPending} disabled={tried && (Object.keys(problems).length > 0 || nothing)} disabledWhy={Object.values(problems)[0] ?? (nothing ? "Nothing has changed." : undefined)} onSubmit={submit} testId="trigger-edit-form">
      <FormSection title="Event source mapping configuration" testId="section-trigger">
        <Field label="SQS queue" help={<>An existing mapping cannot change its source: UpdateEventSourceMapping takes no event source.{holds ? <Muted> {mapping.name} holds {holds.available.toLocaleString("en-US")} {holds.available === 1 ? "message" : "messages"}.</Muted> : null}</>} testId="field-queue">
          {(id) => <Input id={id} value={mapping.source ?? mapping.name} readOnly disabled className="font-mono" data-testid="trigger-queue" />}
        </Field>
        <div className="flex items-start gap-3">
          <Switch id="trigger-enabled" checked={enabled} onCheckedChange={setEnabled} className="mt-1" data-testid="trigger-enabled" />
          <Label htmlFor="trigger-enabled" className="grid gap-0.5 font-normal"><span className="font-medium text-bright">Enable trigger</span><span className="text-[13px] text-muted-foreground">When cleared, Lambda pauses polling and invocation; the messages stay in the queue.</span></Label>
        </div>
        <Field label="Batch size" error={tried ? problems.batch : undefined} help={<>The maximum number of records to send to the function in each batch: up to <b>{batchMost(mapping.source ?? null).toLocaleString("en-US")}</b> {mapping.source?.endsWith(".fifo") ? "for a FIFO queue" : "for a standard queue and 10 for a FIFO queue"}, default <b>10</b>.{Number(batch) !== from.batch ? <Muted> Changed from {from.batch}.</Muted> : null}</>} testId="field-batch">
          {(id) => <Input id={id} value={batch} onChange={(e) => setBatch(e.target.value)} inputMode="numeric" className="max-w-[16rem] font-mono" data-testid="trigger-batch" />}
        </Field>
      </FormSection>
      <FormSection title={EXTRA} collapsible defaultOpen={false} description="Optional. These settings are changed with the AWS CLI or an SDK; this form leaves them as they are." testId="section-more">
        <p className="text-[13.5px] text-muted-foreground">The mapping keeps the batch window, partial batch responses, maximum concurrency and filter criteria it has. Changing them is UpdateEventSourceMapping with those members.</p>
      </FormSection>
    </FnFormPage>
  );
}

type Type = Destination["type"];
const TYPES: { id: Type; label: string }[] = [
  { id: "sqs", label: "Amazon SQS queue" },
  { id: "sns", label: "Amazon SNS topic" },
  { id: "lambda", label: "Lambda Event Function" },
];

export function DestinationAdd(p: FormProps) {
  const { d, where } = p;
  const [condition, setCondition] = useState<Destination["condition"]>("OnFailure");
  const [type, setType] = useState<Type>("sqs");
  const [target, setTarget] = useState<string | null>(null);
  const [tried, setTried] = useState(false);
  const current = useOpRead<{ Absent?: string; DestinationConfig?: Record<string, { Destination?: string }> }>(on.fn(d.name), "GetFunctionEventInvokeConfig", {});
  const update = useOp(on.fn(d.name), "UpdateFunctionEventInvokeConfig");
  const put = useOp(on.fn(d.name), "PutFunctionEventInvokeConfig");
  const exists = Boolean(current.data) && !current.data?.Absent && !notFound(current.error);
  const arn = target === null ? null : type === "sqs" ? arnOf(where, target) : type === "lambda" ? `arn:aws:lambda:${where.region}:${where.account}:function:${target}` : target;
  const dest: Destination = { condition, type, target: arn };
  const problems = destinationProblems(dest);
  const input = destinationInput(d.name, dest);
  const op = exists ? "UpdateFunctionEventInvokeConfig" : "PutFunctionEventInvokeConfig";
  const submit = () => {
    setTried(true);
    if (Object.keys(problems).length > 0) return;
    (exists ? update : put).mutate({ ...input }, { onSuccess: () => p.go({ tab: "configuration", p: "triggers", done: "destination-saved" }) });
  };
  const fifoWord = target && (target.endsWith(".fifo") || target.includes(".fifo")) ? target : null;
  return (
    <FnFormPage p={{ ...p, tab: p.tab ?? "configuration", p: "triggers" }} title="Add destination" label="Add destination" sub={<><b className="font-medium text-foreground">Keep a record of asynchronous invocations.</b> A destination receives a JSON record when an event succeeds or fails every attempt.</>}
      request={target ? { service: "lambda", operation: op, input } : null} refusal={update.error ?? put.error} callNote={exists ? "Update changes only the member you send. Put would replace the whole async configuration." : `${d.name} has no asynchronous configuration yet, so the first one is made with Put; Update would refuse it.`}
      submitLabel="Save" pending={update.isPending || put.isPending} disabled={tried && Object.keys(problems).length > 0} disabledWhy={Object.values(problems)[0]} onSubmit={submit} testId="destination-form">
      <FormSection title="Destination configuration" testId="section-destination">
        <Field label="Source" help={<>Async events come from SNS, S3 and EventBridge. {p.triggers.some((t) => t.kind === "SNS topic") ? <Muted>The {p.triggers.find((t) => t.kind === "SNS topic")?.name} topic invokes {d.name} this way.</Muted> : null}</>} testId="field-source">
          {() => <div className="flex h-9 w-full max-w-[22rem] items-center rounded-md border bg-card px-3 text-[14px] text-bright" data-testid="dsource">Asynchronous invocation</div>}
        </Field>
        <div className="grid gap-1.5">
          <Label>Condition</Label>
          <RadioGroup value={condition} onValueChange={(v) => setCondition(v as Destination["condition"])} className="grid gap-3" aria-label="Condition">
            {[["OnFailure", "On failure", "Send a record when the event fails all processing attempts or exceeds the maximum age."], ["OnSuccess", "On success", "Send a record when the function successfully processes an asynchronous invocation."]].map(([v, t, text]) => (
              <Label key={v} htmlFor={`cond-${v}`} className="flex cursor-pointer items-start gap-3 font-normal leading-5">
                <RadioGroupItem id={`cond-${v}`} value={v!} className="mt-0.5" data-testid={`condition-${v}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{t}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
              </Label>
            ))}
          </RadioGroup>
        </div>
        <div className="grid gap-1.5">
          <Label>Destination type</Label>
          <RadioGroup value={type} onValueChange={(v) => { setType(v as Type); setTarget(null); }} className="grid gap-2" aria-label="Destination type">
            {TYPES.map((t) => (
              <Label key={t.id} htmlFor={`type-${t.id}`} className="flex cursor-pointer items-center gap-3 font-normal">
                <RadioGroupItem id={`type-${t.id}`} value={t.id} data-testid={`dtype-${t.id}`} />{t.label}
              </Label>
            ))}
            {[["EventBridge", "Not available in doze-aws: Lambda does not deliver invocation records to an event bus here."], [`Amazon S3 bucket (on failure only)`, "Not available in doze-aws: Lambda does not deliver invocation records to S3 here."]].map(([t, why]) => (
              <div key={t} className="flex items-start gap-3 text-[13.5px] opacity-70" data-testid="dtype-off"><RadioGroupItem value={`off-${t}`} disabled className="mt-0.5" /><span className="grid gap-0.5"><span className="text-bright">{t}</span><span className="text-muted-foreground">{why}</span></span></div>
            ))}
          </RadioGroup>
        </div>
        <Field label="Destination" error={tried ? problems.target : fifoWord ? problems.target : undefined} help={<>Only standard queues and topics work. {fifoWord ? <Muted><span className="font-mono">{fifoWord.split(/[:/]/).pop()}</span> is a FIFO queue, so it cannot be a destination. </Muted> : null}<Muted>The execution role needs {type === "sqs" ? "sqs:SendMessage on the queue" : type === "sns" ? "sns:Publish on the topic" : "lambda:InvokeFunction on the function"}.</Muted></>} testId="field-target">
          {(id) => type === "sqs" ? <QueuePicker id={id} value={target} onChange={setTarget} by="name" className="w-full max-w-[22rem] font-mono" /> : type === "sns" ? <TopicPicker id={id} value={target} onChange={setTarget} className="w-full max-w-[22rem] font-mono" /> : <FunctionPicker id={id} value={target} onChange={setTarget} by="name" className="w-full max-w-[22rem] font-mono" />}
        </Field>
      </FormSection>
    </FnFormPage>
  );
}
