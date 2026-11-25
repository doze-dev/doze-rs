// The first sections of Create topic and Edit topic: type and name, FIFO options, maximum message size, encryption and access policy.
import { useQuery } from "@tanstack/react-query";
import { Combobox } from "@doze/ui/components/app/combobox";
import { checkJson, JsonEditor } from "@doze/ui/components/app/json-editor";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Switch } from "@doze/ui/components/ui/switch";
import { useResourceDetail } from "../../../api/resources";
import { callKindOp, Field, FormSection, LocalStatus, RuleHelp } from "../../../kit";
import { count, list } from "./words";
import { defaultPolicy, isDefaultPolicy, type Problems, type TopicForm } from "./topic-form-model";

export type FieldProps = { f: TopicForm; set: (patch: Partial<TopicForm>) => void; p: Problems; /** Problems are said once the person has tried to save. */ show: boolean };

const TYPES: [boolean, string, string][] = [
  [false, "Standard", "Amazon SQS queues, AWS Lambda functions and HTTP/S endpoints deliver locally."],
  [true, "FIFO", "The name ends with .fifo. Subscribe Amazon SQS queues; messages in a message group are delivered in the order they are published."],
];

/** What the Region already has, said under the fields it is a neighbour of. */
export type Hints = { topics: string[]; fifoQueues: string[]; region: string };

/** Type and name (create), or the two said as fixed (edit); the display name. `refusal` is the service's word on the name. */
export function Details({ f, set, p, show, create, hints, refusal, mono }: FieldProps & { create: boolean; hints?: Hints; refusal?: { why: React.ReactNode; suggestion?: React.ReactNode } | null; mono?: boolean }) {
  const queue = hints?.fifoQueues.includes(f.name);
  const fifoNote = f.fifo ? (queue ? <>Subscribe Amazon SQS queues to a FIFO topic. A FIFO queue called <b className="font-semibold text-foreground">{f.name}</b> already exists in SQS.</> : hints && hints.fifoQueues.length > 0 ? <>Subscribe Amazon SQS queues to a FIFO topic. FIFO queues in SQS: {list(hints.fifoQueues.slice(0, 3))}.</> : <>Subscribe Amazon SQS queues to a FIFO topic; SQS has no FIFO queue yet.</>) : null;
  return (
    <FormSection title="Details" testId="section-details">
      {create ? (
        <div className="grid gap-1.5">
          <Label>Type</Label>
          <RadioGroup value={f.fifo ? "fifo" : "standard"} onValueChange={(v) => set({ fifo: v === "fifo" })} className="grid gap-3 sm:grid-cols-2" aria-label="Topic type">
            {TYPES.map(([fifo, title, text]) => (
              <Label key={title} htmlFor={`type-${title}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem id={`type-${title}`} value={fifo ? "fifo" : "standard"} className="mt-0.5" data-testid={`type-${fifo ? "fifo" : "standard"}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{title}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
              </Label>
            ))}
          </RadioGroup>
          {fifoNote && <p className="text-[13px] leading-5 text-muted-foreground" data-testid="fifo-note">{fifoNote}</p>}
          <RuleHelp><b>The type and the name are fixed.</b> After the topic exists you cannot change either. An account can create at most 100,000 standard topics and 1,000 FIFO topics.</RuleHelp>
        </div>
      ) : (
        <>
          <Field label="Type" testId="field-type">{(id) => <Input id={id} value={f.fifo ? "FIFO" : "Standard"} readOnly className="bg-muted text-muted-foreground" data-testid="edit-type" />}</Field>
          <Field label="Name" testId="field-name-fixed">{(id) => <Input id={id} value={f.name} readOnly className={mono ? "bg-muted font-mono text-muted-foreground" : "bg-muted text-muted-foreground"} data-testid="edit-name" />}</Field>
        </>
      )}
      {create && (
        <Field label="Name" refusal={refusal} error={show ? p.name : undefined} help={f.fifo ? "1 to 256 characters: letters, numbers, underscores and hyphens. A FIFO name must end with .fifo." : "1 to 256 characters: uppercase and lowercase ASCII letters, numbers, underscores and hyphens."} hint={f.fifo && f.name.endsWith(".fifo") ? <>The .fifo suffix is part of the name. The ARN is <span className="font-mono text-foreground">{`arn:aws:sns:${hints?.region ?? "us-east-1"}:…:${f.name}`}</span>.</> : hints && hints.topics.length > 0 ? <><b className="font-semibold text-foreground">Creating a name you already own is not an error.</b> CreateTopic returns the existing topic's ARN. {list(hints.topics.slice(0, 3))} already {hints.topics.length === 1 ? "exists" : "exist"} in {hints.region}.</> : undefined} testId="field-name">
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder={f.fifo ? "orders-events.fifo" : "deliveries-updated"} autoComplete="off" spellCheck={false} className="font-mono" data-testid="topic-name" />}
        </Field>
      )}
      <Field label="Display name" optional error={show ? p.display : undefined} help={<>For email and email-json subscriptions, the sender name of regular notification messages. Attribute: <span className="font-mono">DisplayName</span>.</>} hint={undefined} testId="field-display">
        {(id) => <Input id={id} value={f.display} onChange={(e) => set({ display: e.target.value })} placeholder="Add a display name" autoComplete="off" className="bg-card" data-testid="topic-display" />}
      </Field>
      <LocalStatus state="stored" reason="doze-aws sends no email." className="-mt-3" />
    </FormSection>
  );
}

/** Content-based deduplication and the throughput scope of a FIFO topic. */
export function FifoOptions({ f, set }: Pick<FieldProps, "f" | "set">) {
  return (
    <FormSection title="FIFO options" testId="section-fifo">
      <div className="flex items-start gap-3" data-testid="field-dedup">
        <Switch id="topic-dedup" checked={f.dedup} onCheckedChange={(dedup) => set({ dedup })} className="mt-0.5" data-testid="topic-dedup" />
        <div className="grid gap-1">
          <Label htmlFor="topic-dedup">Content-based message deduplication</Label>
          <RuleHelp>SNS hashes the message body (SHA-256, attributes excluded) to make the deduplication ID. Attribute: <span className="font-mono">ContentBasedDeduplication</span>, false by default.</RuleHelp>
        </div>
      </div>
      <div className="grid gap-1.5">
        <Label>FifoThroughputScope</Label>
        <RadioGroup value={f.scope} onValueChange={(v) => set({ scope: v as TopicForm["scope"] })} className="grid gap-2" aria-label="FIFO throughput scope">
          {([["Topic", "The scope of message deduplication is across the entire topic. Default. A maximum throughput of 3,000 messages per second or 20 MB per second, whichever comes first."], ["MessageGroup", "The scope of deduplication is within each individual message group, which enables higher throughput per topic, subject to regional quotas."]] as const).map(([v, text]) => (
            <Label key={v} htmlFor={`scope-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
              <RadioGroupItem id={`scope-${v}`} value={v} className="mt-0.5" data-testid={`scope-${v}`} />
              <span className="grid gap-0.5"><span className="font-medium text-bright">{v}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
            </Label>
          ))}
        </RadioGroup>
        <RuleHelp>Two possible values: <b>Topic</b> and <b>MessageGroup</b>.</RuleHelp>
      </div>
      <RuleHelp>With content-based deduplication off, every publish must carry its own deduplication ID. Messages with the same ID inside the 5 minute interval are accepted but delivered once.</RuleHelp>
    </FormSection>
  );
}

/** The most a message may be; what a size above 256 KiB asks of the subscriptions. */
export function MaxSize({ f, set, p, show, collapsed, own }: FieldProps & { collapsed: boolean; own?: { total: number; sqsOrLambda: number } }) {
  const kib = Number(f.maxKib);
  const summary = f.maxKib.trim() === "256" ? "256 KiB (default)" : `${f.maxKib} KiB`;
  const big = kib > 256;
  return (
    <FormSection title="Maximum message size" collapsible={collapsed} defaultOpen={!collapsed} summary={summary} testId="section-size">
      <Field label="Maximum message size" optional error={show ? p.maxKib : undefined} help={<><span className="font-mono">MaximumMessageSize</span>: 1 KiB to 1,024 KiB. The default is 256 KiB (262,144 bytes).</>} testId="field-max-size">
        {(id) => (
          <div className="flex items-center gap-2">
            <Input id={id} value={f.maxKib} onChange={(e) => set({ maxKib: e.target.value })} inputMode="numeric" className="w-40 bg-card font-mono" autoComplete="off" data-testid="topic-max-size" />
            <span className="text-[13.5px] text-muted-foreground">KiB</span>
          </div>
        )}
      </Field>
      <RuleHelp><b>Above 256 KiB, the topic is limited to 100 subscriptions,</b> each Amazon SQS or AWS Lambda here.{own ? (big ? <> This topic has {count(own.total, "subscription")}, {own.total <= 100 ? "under" : "over"} the limit of 100, and {own.total === 0 ? "none" : own.sqsOrLambda === own.total ? (own.total === 1 ? "it is" : "all of them are")  : `${own.total - own.sqsOrLambda} are not`} Amazon SQS or AWS Lambda. SNS refuses the change with InvalidParameter if that stops being true.</> : <> It has {count(own.total, "subscription")} now.</>) : null}</RuleHelp>
    </FormSection>
  );
}

function useAliases() {
  return useQuery({
    queryKey: ["sns", "aliases"],
    queryFn: async () => {
      const r = await callKindOp<{ Aliases?: { AliasName: string }[] }>({ service: "sns", kind: "topics" }, "ListAliases", {});
      return (r.Aliases ?? []).map((a) => a.AliasName).sort();
    },
    retry: false,
  });
}

/** Server-side encryption with a KMS key. Stored, not applied: nothing is encrypted. */
export function Encryption({ f, set, p, show, collapsed }: FieldProps & { collapsed: boolean }) {
  const aliases = useAliases();
  const key = useResourceDetail("kms", "keys", f.kms, f.sse && f.kms.startsWith("alias/"));
  const fact = (label: string) => key.data?.fields.find((x) => x.label === label)?.value;
  const facts = key.data ? [fact("Key spec") === "SYMMETRIC_DEFAULT" ? "symmetric" : fact("Key spec")?.toLowerCase(), fact("State")?.toLowerCase()].filter(Boolean).join(" and ") : null;
  return (
    <FormSection title="Encryption" collapsible={collapsed} defaultOpen={!collapsed} description="Server-side encryption with AWS KMS." summary={f.sse ? `On, with ${f.kms || "no key chosen"}.` : "Off"} testId="section-encryption">
      <div className="flex items-start gap-3">
        <Switch id="sse-on" checked={f.sse} onCheckedChange={(sse) => set({ sse })} className="mt-0.5" data-testid="sse-on" />
        <div className="grid gap-0.5">
          <Label htmlFor="sse-on">Enable encryption</Label>
          <span className="text-[13px] text-muted-foreground">Server-side encryption with AWS KMS.</span>
        </div>
      </div>
      {f.sse && (
        <>
          <Field label="KMS key" error={show ? p.kms : undefined} help={<>You can also enter the ARN of a key in this or another account. Attribute: <span className="font-mono">KmsMasterKeyId</span>.</>} hint={facts ? `${f.kms} is ${facts}.` : undefined} testId="field-kms">
            {(id) => <Combobox id={id} options={(aliases.data ?? []).map((a) => ({ value: a, label: a }))} value={f.kms || null} onChange={(kms) => set({ kms: kms ?? "" })} placeholder={aliases.isPending ? "Reading keys…" : "Choose a KMS key"} search="Search aliases, or type an alias or ARN…" empty="No alias matches." className="w-full font-mono" pattern aria-label="KMS key" />}
          </Field>
          <LocalStatus state="stored" reason={<>The key is recorded on the topic; nothing is encrypted here. On AWS a custom key needs a key policy that lets sns.amazonaws.com call kms:Decrypt and kms:GenerateDataKey.</>} />
        </>
      )}
    </FormSection>
  );
}

/** Who may publish and subscribe: the policy as JSON. */
export function AccessPolicy({ f, set, p, name, region, account, collapsed }: FieldProps & { name: string; region: string; account: string; collapsed: boolean }) {
  const standard = defaultPolicy(name, region, account);
  const same = isDefaultPolicy(f.policy, standard);
  return (
    <FormSection title="Access policy" collapsible={collapsed} defaultOpen={!collapsed} description="By default, only the topic owner can publish or subscribe." summary={same ? "Default: only the topic owner can publish or subscribe" : "A custom policy."} testId="section-access">
      <JsonEditor value={f.policy === "" ? standard : f.policy} onChange={(policy) => set({ policy })} label="Policy optional" rows={18} testId="policy-json" help={<>When you create a topic in the console, the default policy uses the aws:SourceOwner condition key, similar to aws:SourceAccount. Attribute: <span className="font-mono">Policy</span>.</>} />
      {/* The editor says what is not JSON; this says only what its words do not (valid JSON that is not an object). */}
      {p.policy && checkJson(f.policy).ok && <p className="text-[13px] text-bad" role="alert">{p.policy}</p>}
    </FormSection>
  );
}
