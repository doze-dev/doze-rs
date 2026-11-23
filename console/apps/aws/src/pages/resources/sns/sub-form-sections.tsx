// The sections of Create subscription and Edit subscription: details (topic, protocol, endpoint, raw delivery), the filter policy and the
// redrive policy. Redrive and payload-based filtering are applied by doze-aws, which the boards say they are not (board-deviations.md).
import { ChevronDownIcon } from "lucide-react";
import { useState } from "react";
import { Combobox } from "@doze/ui/components/app/combobox";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { Badge } from "@doze/ui/components/ui/badge";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Switch } from "@doze/ui/components/ui/switch";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormSection, RuleHelp } from "../../../kit";
import { countWord, disabledCount, PROTOCOLS, protocolOf, type Protocol } from "./protocols";
import type { Problems, SubForm } from "./sub-form-model";
import { list } from "./words";

export type SubFieldProps = { f: SubForm; set: (patch: Partial<SubForm>) => void; p: Problems; show: boolean };

/** The rows of a kind of another service, read once for the hints and pickers. */
export function useRows(service: string, kind: string) {
  const q = useResourceList(service, kind);
  return { rows: q.data?.pages.flatMap((pg) => pg.rows) ?? [], pending: q.isPending };
}

/** Protocol: a radio for each, with what it is under it and, for the ones doze-aws does not deliver to, why it is off (greyed, not hidden:
 * someone following the AWS documentation looks for it). */
function ProtocolSelect({ f, set }: Pick<SubFieldProps, "f" | "set">) {
  const info = protocolOf(f.protocol);
  // The list is open on arrival (nine choices, five of them off, each saying why) and folds to the chosen protocol once one is chosen
  // or the page was opened for one (the HTTP and Lambda forms), as the select of the boards does.
  const [open, setOpen] = useState(true);
  const [seen, setSeen] = useState(f.protocol);
  if (seen !== f.protocol) { setSeen(f.protocol); setOpen(false); }
  return (
    <Field label="Protocol" help={info?.active ? info.said ? <><b className="font-semibold text-foreground">{info.said.lead}</b> {info.said.more}</> : <>{info.about}</> : undefined} hint={<><b className="font-semibold text-foreground">{f.topic.endsWith(".fifo") ? "To subscribe to a FIFO topic, choose Amazon SQS. " : ""}</b>{countWord(disabledCount())} of the nine protocols are disabled here; each says why under its name.</>} testId="field-protocol">
      {(id) => (
        <div className="grid gap-2">
        <button type="button" id={id} onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-haspopup="listbox" className="flex h-9 w-full items-center justify-between rounded-md border bg-card px-3 text-left text-[14px] text-bright" data-testid="protocol-trigger">
          <span>{info?.label ?? "Choose a protocol"}</span><ChevronDownIcon aria-hidden className={`size-4 text-muted-foreground transition-transform ${open ? "rotate-180" : ""}`} />
        </button>
        {open && <RadioGroup value={f.protocol} onValueChange={(v) => { set({ protocol: v as Protocol, endpoint: "", raw: false }); setOpen(false); }} className="grid gap-1 rounded-xl border bg-card p-2" aria-label="Protocol" data-testid="protocol-list">
          {PROTOCOLS.map((x) => (
            <Label key={x.id} htmlFor={`protocol-radio-${x.id}`} data-testid={`protocol-${x.id}`} data-disabled={x.active ? undefined : ""} className={`flex items-start gap-3 rounded-lg px-3 py-2.5 font-normal leading-5 has-[[data-state=checked]]:bg-pick/50 ${x.active ? "cursor-pointer hover:bg-accent/40" : "cursor-not-allowed opacity-60"}`}>
              <RadioGroupItem id={`protocol-radio-${x.id}`} value={x.id} disabled={!x.active} className="mt-0.5" data-testid={`protocol-radio-${x.id}`} />
              <span className="grid gap-0.5">
                <span className="flex items-center gap-2 font-medium text-bright">{x.label}{!x.active && <Badge variant="secondary" className="font-normal text-muted-foreground">Disabled</Badge>}</span>
                <span className="max-w-[40rem] text-[13px] text-muted-foreground">{x.about}</span>
              </span>
            </Label>
          ))}
        </RadioGroup>}
        </div>
      )}
    </Field>
  );
}

/** The endpoint: a queue or a function chosen from the account's (as the ARN), or typed; a URL as it is. */
function EndpointField({ f, set, p, show }: SubFieldProps) {
  const about = useAbout().data;
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const queues = useRows("sqs", "queues");
  const fns = useRows("lambda", "functions");
  const subs = useRows("sns", "subscriptions");
  const fifoTopic = f.topic.endsWith(".fifo");
  const chosenName = f.endpoint.split(":").pop() ?? "";
  const arnOf = (kind: "sqs" | "lambda", name: string) => (kind === "sqs" ? `arn:aws:sqs:${region}:${account}:${name}` : `arn:aws:lambda:${region}:${account}:function:${name}`);
  // Only the queues that fit the topic are offered (a FIFO topic takes FIFO queues, a standard one standard queues); another is still accepted when its ARN is typed.
  const options = f.protocol === "sqs" ? queues.rows.filter((r) => (r.cells[0] === "FIFO") === fifoTopic || r.name === chosenName).map((r) => ({ value: arnOf("sqs", r.name), label: arnOf("sqs", r.name) })) : f.protocol === "lambda" ? fns.rows.map((r) => ({ value: arnOf("lambda", r.name), label: arnOf("lambda", r.name) })) : null;
  const chosen = queues.rows.find((r) => r.name === (f.endpoint.split(":").pop() ?? ""));
  const misfits = queues.rows.filter((r) => (r.cells[0] === "FIFO") !== fifoTopic && r.name !== chosen?.name).map((r) => r.name);
  const name = f.endpoint.split(":").pop() ?? "";
  const runtime = f.protocol === "lambda" ? (fns.rows.find((r) => r.name === name)?.cells[2] ?? "") : "";
  const used = f.protocol === "lambda" ? subs.rows.filter((r) => r.cells[2] === f.endpoint).map((r) => r.cells[0] ?? "") : [];
  const info = protocolOf(f.protocol);
  const hint =
    f.protocol === "sqs" && queues.rows.length > 0 ? <><b className="font-semibold text-foreground">{fifoTopic ? "FIFO topics use FIFO queues." : "Standard topics use standard queues."}</b>{chosen ? ` ${name} is a ${chosen.cells[0] === "FIFO" ? "FIFO" : "standard"} queue${(chosen.cells[0] === "FIFO") === fifoTopic ? "" : " and does not fit this topic"}.` : ""}{misfits.length > 0 ? ` ${list(misfits.slice(0, 3))} ${misfits.length === 1 ? "is a" : "are"} ${fifoTopic ? "standard" : "FIFO"} ${misfits.length === 1 ? "queue and fits" : "queues and fit"} ${fifoTopic ? "standard" : "FIFO"} topics only.` : ""}</>
    : f.protocol === "lambda" ? <>String.Array and Number message attributes reach Lambda as String in the JSON payload.{used.length > 0 ? ` ${name}${runtime ? ` (${runtime})` : ""} is already triggered by the ${list(used)} ${used.length === 1 ? "topic" : "topics"}.` : ""}</>
    : f.protocol === "http" || f.protocol === "https" ? <>For the {f.protocol} protocol the endpoint is a URL beginning with {f.protocol}://. The webhook can be a local server.</>
    : undefined;
  return (
    <>
      <Field label="Endpoint" error={show ? p.endpoint : undefined} help={f.protocol === "sqs" ? "For Amazon SQS the endpoint is the ARN of the queue." : f.protocol === "lambda" ? "For the lambda protocol the endpoint is the ARN of a Lambda function." : undefined} hint={hint} testId="field-endpoint">
        {(id) => options ? <Combobox id={id} options={options} value={f.endpoint || null} onChange={(endpoint) => set({ endpoint: endpoint ?? "" })} placeholder={f.protocol === "sqs" ? "Choose a queue, or enter its ARN" : "Choose a function, or enter its ARN"} search={f.protocol === "sqs" ? "Search queues, or enter an ARN…" : "Search functions, or enter an ARN…"} empty="Nothing matches." className="w-full font-mono" pattern aria-label="Endpoint" /> : <Input id={id} value={f.endpoint} onChange={(e) => set({ endpoint: e.target.value })} placeholder={f.protocol === "http" ? "http://127.0.0.1:9000/in" : f.protocol === "https" ? "https://example.com/hook" : "Endpoint"} className="bg-card font-mono" autoComplete="off" spellCheck={false} data-testid="sub-endpoint" />}
      </Field>
      {f.protocol === "sqs" && <RuleHelp>The queue's access policy must let this topic send to it. With IAM enforcement on, a queue without that statement is refused delivery; in soft mode the delivery is made and a warning is logged.</RuleHelp>}
      {info?.confirms === "endpoint" && <RuleHelp><b>The endpoint must confirm.</b> SNS sends a confirmation message that contains a SubscribeURL; the subscription stays pending until the endpoint sends an HTTP GET request to the SubscribeURL or calls ConfirmSubscription. The token is valid for two days.</RuleHelp>}
    </>
  );
}

/** Topic, protocol, endpoint, raw delivery. `create`: the topic and the protocol can be chosen; an edit shows them fixed (`fixed`). */
export function Details({ f, set, p, show, topicHint, locked, after }: SubFieldProps & { topicHint?: React.ReactNode; locked?: React.ReactNode; /** Under the raw delivery choice. */ after?: React.ReactNode }) {
  const topics = useRows("sns", "topics");
  const info = protocolOf(f.protocol);
  return (
    <FormSection title="Details" testId="section-details">
      {locked ? <div className="grid gap-3">{locked}</div> : (
        <>
          <Field label="Topic ARN" error={show ? p.topic : undefined} help="The ARN of the topic you want to subscribe to. Prefilled from the topic you came from." hint={topicHint} testId="field-topic">
            {(id) => <Combobox id={id} options={topics.rows.map((r) => ({ value: r.id, label: r.id }))} value={f.topic || null} onChange={(topic) => set({ topic: topic ?? "" })} placeholder={topics.pending ? "Reading topics…" : "Choose a topic, or enter its ARN"} search="Search topics, or enter an ARN…" empty="No topic matches." className="w-full font-mono" pattern aria-label="Topic ARN" />}
          </Field>
          <ProtocolSelect f={f} set={set} />
          <EndpointField f={f} set={set} p={p} show={show} />
        </>
      )}
      {info?.raw && info.active && (
        <div className="flex items-start gap-3" data-testid="field-raw">
          <Checkbox id="sub-raw" checked={f.raw} onCheckedChange={(v) => set({ raw: v === true })} className="mt-0.5" data-testid="sub-raw" />
          <div className="grid gap-0.5">
            <Label htmlFor="sub-raw">Enable raw message delivery</Label>
            <span className="text-[13px] leading-5 text-muted-foreground">{f.protocol === "sqs" ? "For Amazon SQS and HTTP/S endpoints. SNS metadata is stripped from the published message and it is sent as is." : "For HTTP/S endpoints, SNS adds the HTTP header x-amz-sns-rawdelivery with the value true, which indicates the message was published without JSON formatting. Message attributes are not sent."}</span>
          </div>
        </div>
      )}
      {after}
    </FormSection>
  );
}

/** The filter policy: deliver only the messages that match, by their attributes or by their body. Both scopes are applied by doze-aws
 * (crates/aws-sns/src/filter.rs: the policy is parsed for the scope it has). */
export function FilterPolicy({ f, set, p, collapsed }: SubFieldProps & { collapsed: boolean }) {
  return (
    <FormSection title="Subscription filter policy" collapsible={collapsed} defaultOpen={!collapsed} description="Optional. Deliver only the messages that match." summary={f.filter.trim() !== "" ? "A filter policy: only the messages that match are delivered." : "No filter policy: every message published to the topic is delivered"} testId="section-filter">
      {(
        <>
          <div className="grid gap-1.5">
            <Label>Filter policy scope</Label>
            <RadioGroup value={f.scope} onValueChange={(v) => set({ scope: v as SubForm["scope"] })} className="grid gap-2" aria-label="Filter policy scope">
              {([["MessageAttributes", "Attribute-based filtering", "The default: the filter is applied on the message attributes."], ["MessageBody", "Payload-based filtering", "The filter is applied on the message body."]] as const).map(([v, title, text]) => (
                <Label key={v} htmlFor={`scope-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                  <RadioGroupItem id={`scope-${v}`} value={v} className="mt-0.5" data-testid={`scope-${v}`} />
                  <span className="grid gap-0.5"><span className="font-medium text-bright">{title}</span><span className="text-[13px] text-muted-foreground">FilterPolicyScope = {v}. {text}</span></span>
                </Label>
              ))}
            </RadioGroup>
            <RuleHelp>Attribute: <span className="font-mono">FilterPolicyScope</span>.</RuleHelp>
          </div>
          <JsonEditor value={f.filter} onChange={(filter) => set({ filter })} label="JSON editor" rows={8} placeholder={'{\n  "event": ["order_placed", "order_cancelled"]\n}'} testId="filter-json" help={<><b className="font-semibold text-foreground">A policy has at most 5 keys and 150 combinations of values</b> (multiply the number of values in each array), and at most 256 KB. String matching is case-sensitive. On AWS a change can take up to 15 minutes to take full effect.</>} />
          {p.filter && <p className="text-[13px] text-bad" role="alert">{p.filter}</p>}
        </>
      )}
    </FormSection>
  );
}

/** The redrive policy: the queue that holds what SNS could not deliver (applied by doze-aws: crates/aws-sns/src/dead_letter.rs). */
export function Redrive({ f, set, p, show, collapsed }: SubFieldProps & { collapsed: boolean }) {
  const about = useAbout().data;
  const queues = useRows("sqs", "queues");
  const arn = (n: string) => `arn:aws:sqs:${about?.region ?? "us-east-1"}:${about?.account ?? "000000000000"}:${n}`;
  const fifoTopic = f.topic.endsWith(".fifo");
  const fit = queues.rows.filter((r) => (r.cells[0] === "FIFO") === fifoTopic);
  const chosen = f.dlq.split(":").pop() ?? "";
  const kind = queues.rows.find((r) => r.name === chosen)?.cells[0] === "FIFO" ? "a FIFO queue" : "a standard queue";
  return (
    <FormSection title="Redrive policy (dead-letter queue)" collapsible={collapsed} defaultOpen={!collapsed} description="Optional. Where messages go when they cannot be delivered." summary={f.redriveOn ? `Enabled: ${chosen || "no queue chosen"}.` : "Disabled."} testId="section-redrive">
      <div className="flex items-center gap-3">
        <Switch id="redrive-on" checked={f.redriveOn} onCheckedChange={(redriveOn) => set({ redriveOn })} data-testid="redrive-on" />
        <div className="grid"><Label htmlFor="redrive-on">{f.redriveOn ? "Enabled" : "Disabled"}</Label><span className="text-[13px] text-muted-foreground">Attribute: RedrivePolicy.</span></div>
      </div>
      {!f.redriveOn && (
        <Field label="deadLetterTargetArn" testId="field-dlq-off">{(id) => <Input id={id} disabled readOnly placeholder="Specify the ARN of an Amazon SQS queue" className="w-full font-mono" data-testid="dlq-off" />}</Field>
      )}
      {f.redriveOn && (
        <>
          <Field label="deadLetterTargetArn" error={show ? p.dlq : undefined} hint={chosen ? `${chosen} is ${kind}.` : undefined} testId="field-dlq">
            {(id) => <Combobox id={id} options={fit.map((r) => ({ value: arn(r.name), label: r.name }))} value={f.dlq || null} onChange={(dlq) => set({ dlq: dlq ?? "" })} placeholder="Specify the ARN of an Amazon SQS queue" search="Search queues, or enter an ARN…" empty="No queue matches." className="w-full font-mono" pattern aria-label="Dead-letter queue" />}
          </Field>
          <RuleHelp><b>Applied by doze-aws.</b> When SNS cannot deliver to this subscription (the queue or function is gone, its policy refuses SNS, an HTTP endpoint keeps failing), it sends the message to this queue. On AWS FIFO topic subscriptions use FIFO queues, and standard topic subscriptions use standard queues. The queue's policy must let sns.amazonaws.com call SQS:SendMessage.</RuleHelp>
        </>
      )}
    </FormSection>
  );
}
