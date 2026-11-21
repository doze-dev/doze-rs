// One target of a rule (eventbridge__rule-create-targets, rule-targets-none's Add target): where it sends and how it is delivered.
import { useState } from "react";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Field, FormSection, FunctionPicker, QueuePicker, ResourcePicker, RadioCards, RuleHelp, TopicPicker, BusPicker } from "../../../kit";
import { busArn, useDests } from "./api";
import { type InputMode, type TargetDraft, type TargetKind, attemptsProblem, ageSeconds, targetProblem } from "./rule-model";
import { TransformerDialog } from "./transformer-dialog";
import { arnName, targetParts } from "./words";

type Group = "event-bus" | "api-destination" | "aws-service";
const SERVICES: { value: TargetKind; label: string }[] = [
  { value: "sqs", label: "SQS queue" }, { value: "sns", label: "SNS topic" }, { value: "lambda", label: "Lambda function" },
  { value: "logs", label: "CloudWatch log group" }, { value: "kinesis", label: "Kinesis stream" }, { value: "states", label: "Step Functions state machine" },
];
const groupOf = (k: TargetKind): Group => (k === "event-bus" ? "event-bus" : k === "api-destination" ? "api-destination" : "aws-service");
const INPUTS: { value: InputMode; title: string; body: string }[] = [
  { value: "event", title: "Matched events", body: "The whole event. The default." },
  { value: "path", title: "Part of the matched events", body: "What a JSON path finds in the event." },
  { value: "constant", title: "Constant (JSON text)", body: "The same JSON for every event." },
  { value: "transformer", title: "Input transformer", body: "A template filled from parts of the event." },
];

/** The ARN of a resource chosen by name for `kind`. */
export const arnFor = (kind: TargetKind, name: string, region: string, account: string): string => {
  switch (kind) {
    case "sqs": return `arn:aws:sqs:${region}:${account}:${name}`;
    case "lambda": return `arn:aws:lambda:${region}:${account}:function:${name}`;
    case "logs": return `arn:aws:logs:${region}:${account}:log-group:${name}`;
    case "kinesis": return `arn:aws:kinesis:${region}:${account}:stream/${name}`;
    case "states": return `arn:aws:states:${region}:${account}:stateMachine:${name}`;
    case "event-bus": return busArn(region, account, name);
    default: return `arn:aws:sns:${region}:${account}:${name}`;
  }
};

function Where({ t, set, region, account }: { t: TargetDraft; set: (p: Partial<TargetDraft>) => void; region: string; account: string }) {
  const dests = useDests();
  const byName = (name: string | null) => set({ arn: name ? arnFor(t.kind, name, region, account) : null });
  const name = t.arn ? arnName(t.arn) : null;
  switch (t.kind) {
    case "sqs": return <QueuePicker by="name" value={name} onChange={byName} />;
    case "sns": return <TopicPicker by="name" value={name} onChange={byName} />;
    case "lambda": return <FunctionPicker by="name" value={name} onChange={byName} />;
    case "logs": return <ResourcePicker service="logs" kind="groups" noun="log group" by="name" value={t.arn ? t.arn.split("log-group:")[1] ?? null : null} onChange={byName} />;
    case "kinesis": return <ResourcePicker service="kinesis" kind="streams" noun="stream" by="name" value={name} onChange={byName} />;
    case "states": return <ResourcePicker service="stepfunctions" kind="machines" noun="state machine" by="name" value={t.arn ? t.arn.split("stateMachine:")[1] ?? null : null} onChange={byName} />;
    case "event-bus": return <BusPicker by="name" value={name} onChange={byName} />;
    default: return <Combobox options={(dests.data?.ApiDestinations ?? []).map((d) => ({ value: d.ApiDestinationArn, label: d.Name }))} value={t.arn} onChange={(arn) => set({ arn })} placeholder={dests.isPending ? "Reading API destinations…" : (dests.data?.ApiDestinations?.length ?? 0) === 0 ? "There are no API destinations yet" : "Choose an API destination"} search="Search API destinations…" empty="No API destination matches." aria-label="API destination" className="w-full" />;
  }
}

export function TargetForm({ t, index, set, remove, region, account, tried }: { t: TargetDraft; index: number; set: (p: Partial<TargetDraft>) => void; remove?: () => void; region: string; account: string; tried: boolean }) {
  const [dialog, setDialog] = useState(false);
  const group = groupOf(t.kind);
  const problem = targetProblem(t);
  const age = ageSeconds(t);
  const attempts = attemptsProblem(t.attempts);
  const parts = t.arn ? targetParts(t.arn) : null;
  const names = Object.keys((() => { try { return (JSON.parse(t.paths || "{}") as Record<string, string>); } catch { return {}; } })());
  const dlqName = t.dlq ? arnName(t.dlq) : null;
  const retry = (p: Partial<TargetDraft>) => set({ ...p, retry: true });
  return (
    <FormSection title={`Target ${index + 1}`} description={index === 0 ? "A rule can have up to 5 targets." : undefined} testId={`section-target-${index + 1}`}>
      <div className="grid gap-1.5">
        <Label>Target types</Label>
        <RadioCards<Group> name={`group-${index}`} label="Target types" value={group} onChange={(g) => set({ kind: g === "aws-service" ? (group === "aws-service" ? t.kind : "sns") : g, arn: null })} options={[{ value: "event-bus", title: "EventBridge event bus", body: "Another bus of this Region: its own rules see the event." }, { value: "api-destination", title: "EventBridge API destination", body: "An HTTP endpoint. Delivered, including to localhost." }, { value: "aws-service", title: "AWS service", body: "SQS queue, SNS topic, Lambda function, CloudWatch log group, Kinesis stream or Step Functions state machine." }]} className="sm:grid-cols-3" />
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        {group === "aws-service" && (
          <div className="grid gap-1.5">
            <Label htmlFor={`service-${index}`}>Select a target</Label>
            <Combobox id={`service-${index}`} options={SERVICES} value={t.kind} onChange={(k) => k && set({ kind: k as TargetKind, arn: null })} aria-label="Select a target" className="w-full" />
          </div>
        )}
        <Field label={group === "api-destination" ? "API destination" : group === "event-bus" ? "Event bus" : (SERVICES.find((s) => s.value === t.kind)?.label ?? "Resource").replace(/^.* /, (w) => w.charAt(0).toUpperCase() + w.slice(1))} error={tried && !t.arn ? "Choose where the target sends." : undefined} testId={`field-where-${index + 1}`}>
          {() => <Where t={t} set={set} region={region} account={account} />}
        </Field>
      </div>
      {parts && <p className="text-[13px] text-muted-foreground" data-testid={`arn-${index + 1}`}><span className="font-mono">{t.arn}</span></p>}
      <RuleHelp>Delivered by doze-aws: SQS, SNS, Lambda, CloudWatch Logs, Kinesis, Step Functions, another event bus of the Region and API destinations. A target on any other service is stored and not delivered. doze-aws assumes no role, so the Execution role section is not drawn.</RuleHelp>
      <FormSection title="Additional settings" description="All optional. The defaults send the whole event, retry for 24 hours and drop what still fails." collapsible defaultOpen className="bg-muted/30" testId={`section-settings-${index + 1}`}>
        <div className="grid gap-1.5">
          <Label>Configure target input</Label>
          <RadioCards<InputMode> name={`input-${index}`} label="Configure target input" value={t.input} onChange={(input) => set({ input })} options={INPUTS} className="sm:grid-cols-4" />
        </div>
        {t.input === "path" && <Field label="Path" error={tried && !t.path.startsWith("$") ? "A path starts with $." : undefined}>{(id) => <Input id={id} value={t.path} onChange={(e) => set({ path: e.target.value })} className="max-w-md font-mono" data-testid={`input-path-${index + 1}`} />}</Field>}
        {t.input === "constant" && <Field label="Constant" error={tried && problem === "The constant is JSON text." ? problem : undefined}>{(id) => <Input id={id} value={t.constant} onChange={(e) => set({ constant: e.target.value })} placeholder='{"source": "larder"}' className="font-mono" data-testid={`input-constant-${index + 1}`} />}</Field>}
        {t.input === "transformer" && (
          <div className="grid gap-2">
            <div><Button type="button" variant="outline" className="bg-card" onClick={() => setDialog(true)} data-testid={`transformer-open-${index + 1}`}>Configure input transformer</Button></div>
            {t.template ? <p className="text-[13.5px]" data-testid={`transformer-set-${index + 1}`}>Transformer set: <b className="font-semibold text-bright">{names.length} {names.length === 1 ? "variable" : "variables"}</b>{names.length > 0 ? `, ${names.join(", ")},` : ""} shape the message sent to <span className="font-mono font-semibold text-bright">{parts?.name ?? "the target"}</span>.</p> : tried && <p className="text-[13px] text-bad" role="alert">Write the template.</p>}
          </div>
        )}
        <div className="grid gap-3" data-testid={`retry-${index + 1}`}>
          <Label>Retry policy</Label>
          <div className="grid gap-3 sm:grid-cols-4">
            {([["hours", "hour"], ["minutes", "min"], ["seconds", "sec"]] as const).map(([k, unit]) => (
              <div key={k} className="grid gap-1.5">
                <Label htmlFor={`age-${k}-${index}`} className="text-[12.5px] font-normal text-muted-foreground">{k === "hours" ? "Maximum age of event" : " "}</Label>
                <div className="relative"><Input id={`age-${k}-${index}`} value={t[k]} onChange={(e) => retry({ [k]: e.target.value })} inputMode="numeric" className="pr-12" data-testid={`age-${k}-${index + 1}`} /><span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">{unit}</span></div>
              </div>
            ))}
            <Field label="Retry attempts"><Input value={t.attempts} onChange={(e) => retry({ attempts: e.target.value })} inputMode="numeric" data-testid={`attempts-${index + 1}`} aria-label="Retry attempts" /></Field>
          </div>
          {(("problem" in age && t.retry) || (attempts && t.retry)) && <p className="text-[13px] text-bad" role="alert" data-testid={`retry-problem-${index + 1}`}>{"problem" in age ? age.problem : attempts}</p>}
          <RuleHelp>By default AWS retries for <b>24 hours</b> and up to <b>185</b> times, with exponential back-off and jitter, then drops the event. The age can be <b>60 seconds to 24 hours</b>, attempts <b>0 to 185</b>. <b>Applied here:</b> a failed delivery is retried with a back-off that doubles from one second to a minute until it is out of tries or too old (<span className="font-mono">crates/aws-eventbridge/src/retry.rs</span>).</RuleHelp>
        </div>
        <div className="grid gap-2" data-testid={`dlq-${index + 1}`}>
          <Label>Dead-letter queue</Label>
          <RadioGroup value={t.dlq ? "queue" : "none"} onValueChange={(v) => set({ dlq: v === "none" ? null : (t.dlq ?? `arn:aws:sqs:${region}:${account}:`) })} aria-label="Dead-letter queue" className="gap-2.5">
            <Label htmlFor={`dlq-none-${index}`} className="font-normal"><RadioGroupItem id={`dlq-none-${index}`} value="none" data-testid={`dlq-none-${index + 1}`} /> None</Label>
            <Label htmlFor={`dlq-queue-${index}`} className="font-normal"><RadioGroupItem id={`dlq-queue-${index}`} value="queue" data-testid={`dlq-queue-${index + 1}`} /> Select an Amazon SQS queue in the current AWS account to use as the dead-letter queue</Label>
            <Label className="font-normal opacity-60"><RadioGroupItem value="other" disabled /> Select an Amazon SQS queue in another AWS account as a dead-letter queue <span className="text-muted-foreground">Not available in doze-aws: one account.</span></Label>
          </RadioGroup>
          {t.dlq !== null && <div className="max-w-md"><QueuePicker by="name" value={dlqName || null} onChange={(n) => set({ dlq: n ? `arn:aws:sqs:${region}:${account}:${n}` : `arn:aws:sqs:${region}:${account}:` })} aria-label="Dead-letter queue" /></div>}
          <RuleHelp>Only standard queues work as a dead-letter queue. On AWS each failed event arrives with the attributes <span className="font-mono">RULE_ARN</span>, <span className="font-mono">TARGET_ARN</span>, <span className="font-mono">ERROR_CODE</span>, <span className="font-mono">ERROR_MESSAGE</span>, <span className="font-mono">RETRY_ATTEMPTS</span> and <span className="font-mono">EXHAUSTED_RETRY_CONDITION</span>. <b>Applied here:</b> doze-aws sends an event that ran out of tries to the queue with those attributes.</RuleHelp>
        </div>
      </FormSection>
      <div className="flex justify-end">{remove && <Button type="button" variant="ghost" onClick={remove} data-testid={`remove-target-${index + 1}`}>Remove target {index + 1}</Button>}</div>
      <TransformerDialog open={dialog} onOpenChange={setDialog} paths={t.paths} template={t.template} target={parts?.name ?? ""} onConfirm={(paths, template) => set({ paths, template })} />
    </FormSection>
  );
}

