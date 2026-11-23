// The optional sections of Create queue and Edit queue: access policy, redrive allow policy, encryption, dead-letter queue, tags.
import { useQuery } from "@tanstack/react-query";
import { createContext, useContext } from "react";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled } from "@doze/ui/components/app/key-values";
import { Combobox } from "@doze/ui/components/app/combobox";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { listRows } from "./list-model";
import { Field, FormSection, LocalStatus, RuleHelp, Segmented, TAG_RULES, callKindOp } from "../../../kit";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import { basicPolicy, LIMITS, type AllowMode, type QueueForm, type Who } from "./form-model";
import { type FieldProps, TimeInput, timeUnits } from "./form-fields";
import { queueOf, wait } from "./queue-model";

/** The optional sections start folded to their one-line summaries when the form is read again after a refusal (the board's Expand rows). */
export const FoldedStart = createContext(false);

const WHO: [Who, string][] = [["owner", "Only the queue owner"], ["accounts", "Only the specified AWS accounts, IAM users and roles"], ["everyone", "Everyone (public)"]];
const QUEUES = { service: "sqs", kind: "queues" };

function WhoSelect({ id, value, onChange, label }: { id: string; value: Who; onChange: (w: Who) => void; label: string }) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Who)}>
      <SelectTrigger id={id} aria-label={label} className="w-full bg-card" data-testid={id}><SelectValue /></SelectTrigger>
      <SelectContent>
        {WHO.map(([v, t]) => <SelectItem key={v} value={v}>{t}</SelectItem>)}
      </SelectContent>
    </Select>
  );
}

/** Who may reach the queue: Basic (who can send, who can receive, the policy that makes) or Advanced (the JSON). A queue's Edit
 * edits the JSON only: `basic` is false. */
export function AccessPolicy({ f, set, p, queueArn, basic }: FieldProps & { queueArn: string; basic: boolean }) {
  const mode = basic ? f.policyMode : "advanced";
  const resulting = basicPolicy(f, queueArn);
  const owned = f.send === "owner" && f.receive === "owner";
  const summary = f.policyMode === "basic" || !basic ? (resulting && basic ? "A policy is written for the choices made." : f.policy.trim() ? "A custom policy." : "Only the queue owner can send and receive messages (the default).") : "A custom policy.";
  return (
    <FormSection title="Access policy" optional collapsible defaultOpen={!useContext(FoldedStart)} description={basic ? "Defines which accounts, users and roles can reach the queue, and which actions they may take." : "Edit the access policy statements."} summary={summary} testId="section-access">
      {basic && (
        <Segmented value={mode} onChange={(v) => set({ policyMode: v, ...(v === "advanced" && f.policy.trim() === "" && resulting ? { policy: resulting } : {}) })} options={[{ value: "basic", label: "Basic" }, { value: "advanced", label: "Advanced" }]} label="Access policy mode" testId="policy" />
      )}
      {mode === "basic" ? (
        <>
          <Field label="Who can send messages to the queue" testId="field-who-send">{() => <WhoSelect id="policy-send" value={f.send} onChange={(send) => set({ send })} label="Who can send messages to the queue" />}</Field>
          <Field label="Who can receive messages from the queue" testId="field-who-receive">{() => <WhoSelect id="policy-receive" value={f.receive} onChange={(receive) => set({ receive })} label="Who can receive messages from the queue" />}</Field>
          {!owned && (f.send === "accounts" || f.receive === "accounts") && (
            <Field label="AWS accounts, IAM users and roles" hint="One a line: a 12-digit account ID or the ARN of a user or role." testId="field-principals">
              {(id) => <Textarea id={id} value={f.principals} onChange={(e) => set({ principals: e.target.value })} className="min-h-20 bg-card font-mono text-[13px]" spellCheck={false} data-testid="policy-principals" />}
            </Field>
          )}
          <div className="grid gap-1.5" data-testid="resulting-policy">
            <Label>Resulting access policy</Label>
            {resulting ? <CodeBlock text={resulting} max="max-h-64" /> : <p className="rounded-[var(--radius-field)] border border-dashed px-4 py-3 text-[13.5px] text-muted-foreground">No statement is needed: with both choices on Only the queue owner, nobody else can send or receive. The default policy lets only the queue owner send and receive. Anything else, such as letting a service principal send, is written in Advanced.</p>}
            <RuleHelp>Basic shows the resulting policy in a read-only JSON panel. Choose Advanced to edit the JSON directly and name a custom set of actions for each principal.</RuleHelp>
          </div>
        </>
      ) : (
        <div className="grid gap-2">
          <JsonEditor value={f.policy} onChange={(policy) => set({ policy })} label="JSON policy" rows={14} testId="policy-json" help={basic ? undefined : "Saving writes the whole Policy attribute with SetQueueAttributes, so keep every statement you still want."} />
          {p.policy && <p className="text-[13px] text-bad" role="alert">{p.policy}</p>}
          {basic && <RuleHelp>The default policy lets only the queue owner send and receive. Anything else, such as letting a service principal send, is written in Advanced.</RuleHelp>}
        </div>
      )}
    </FormSection>
  );
}

const ALLOW: [AllowMode, string, string][] = [["allowAll", "Allow all", "Any source queue in this account and Region can use this queue as its dead-letter queue."], ["byQueue", "By queue", "Only the source queues you list by ARN, up to 10."], ["denyAll", "Deny all", "No source queue can use this queue as its dead-letter queue."]];

/** Which source queues may name this one as their dead-letter queue (RedriveAllowPolicy). */
export function RedriveAllow({ f, set, p, show, name, note }: FieldProps & { name: string; note?: React.ReactNode }) {
  const shown = f.allowOn ? (f.allowMode === "allowAll" ? "Allow all source queues." : f.allowMode === "byQueue" ? "Only the queues named by ARN." : "Deny all.") : "Disabled. When enabled, the default allows every source queue in this account and Region.";
  return (
    <FormSection title="Redrive allow policy" collapsible defaultOpen={!useContext(FoldedStart)} description="Says which source queues may name THIS queue as their dead-letter queue." summary={shown} testId="section-allow">
      <div className="flex items-center gap-3">
        <Switch id="allow-on" checked={f.allowOn} onCheckedChange={(allowOn) => set({ allowOn })} data-testid="allow-on" />
        <Label htmlFor="allow-on">{f.allowOn ? "Enabled" : "Disabled"}</Label>
      </div>
      {!f.allowOn && <RuleHelp>Leave this off unless other queues should name {name || "this queue"} as their dead-letter queue. When you turn it on, Allow all is the default.</RuleHelp>}
      {note && !f.allowOn && <RuleHelp>{note}</RuleHelp>}
      {f.allowOn && (
        <>
          <RadioGroup value={f.allowMode} onValueChange={(v) => set({ allowMode: v as AllowMode })} className="grid gap-2" aria-label="Redrive permission">
            {ALLOW.map(([v, t, d]) => (
              <Label key={v} htmlFor={`allow-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem id={`allow-${v}`} value={v} className="mt-0.5" data-testid={`allow-${v}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{t}</span><span className="text-[13px] text-muted-foreground">{d}</span></span>
              </Label>
            ))}
          </RadioGroup>
          {f.allowMode === "byQueue" && (
            <Field label="Source queue ARNs" hint="One a line, up to 10." error={show ? p.allow : undefined} testId="field-allow-arns">
              {(id) => <Textarea id={id} value={f.allowArns} onChange={(e) => set({ allowArns: e.target.value })} className="min-h-20 bg-card font-mono text-[13px]" spellCheck={false} data-testid="allow-arns" />}
            </Field>
          )}
          <RuleHelp>Allow all is the default once you enable this. Choose By queue to name up to 10 source queues by ARN; more than 10 need Allow all.</RuleHelp>
        </>
      )}
    </FormSection>
  );
}

/** The aliases of KMS, which the key field offers (and any alias or ARN typed in). */
function useAliases() {
  return useQuery({
    queryKey: ["sqs", "aliases"],
    queryFn: async () => {
      const r = await callKindOp<{ Aliases?: { AliasName: string }[] }>(QUEUES, "ListAliases", {});
      return (r.Aliases ?? []).map((a) => a.AliasName).sort();
    },
    retry: false,
  });
}

/** Server-side encryption: SSE-SQS or SSE-KMS with its key and data key reuse period. Stored, not applied: no message is encrypted. */
export function Encryption({ f, set, p, show, edit }: FieldProps & { edit: boolean }) {
  const aliases = useAliases();
  const on = f.sse !== "none";
  const summary = f.sse === "kms" ? `Enabled with an AWS Key Management Service key (SSE-KMS), ${f.kms || "no key chosen"}.` : f.sse === "sqs" ? "Enabled with an Amazon SQS key (SSE-SQS)." : "Disabled.";
  return (
    <FormSection title="Encryption" optional={edit} collapsible defaultOpen={!useContext(FoldedStart)} description="The server-side encryption settings of the queue." summary={summary} testId="section-encryption">
      <div className="flex items-center gap-3">
        <Switch id="sse-on" checked={on} onCheckedChange={(v) => set({ sse: v ? "sqs" : "none" })} data-testid="sse-on" />
        <Label htmlFor="sse-on">Server-side encryption {on ? "Enabled" : "Disabled"}</Label>
      </div>
      <LocalStatus state="stored" reason="doze-aws keeps the encryption settings and returns them; it encrypts no message." />
      {on && (
        <>
          <RadioGroup value={f.sse} onValueChange={(v) => set({ sse: v as QueueForm["sse"] })} className="grid gap-2" aria-label="Server-side encryption type">
            {[["sqs", "Amazon SQS key (SSE-SQS)", "SQS manages the key for you."], ["kms", "AWS Key Management Service key (SSE-KMS)", "You choose the KMS key."]].map(([v, t, d]) => (
              <Label key={v} htmlFor={`sse-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-3.5 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
                <RadioGroupItem id={`sse-${v}`} value={v!} className="mt-0.5" data-testid={`sse-${v}`} />
                <span className="grid gap-0.5"><span className="font-medium text-bright">{t}</span><span className="text-[13px] text-muted-foreground">{d}</span></span>
              </Label>
            ))}
          </RadioGroup>
          {f.sse === "kms" && (
            <>
              <Field label="KMS key" error={show ? p.kms : undefined} hint="Choose a KMS key alias, or enter the alias or ARN of a key not in the list." testId="field-kms">
                {(id) => <Combobox id={id} options={(aliases.data ?? []).map((a) => ({ value: a, label: a }))} value={f.kms || null} onChange={(kms) => set({ kms: kms ?? "" })} placeholder={aliases.isPending ? "Reading keys…" : "Choose a KMS key"} search="Search aliases, or type an alias or ARN…" empty="No alias matches." className="w-full font-mono" pattern aria-label="KMS key" />}
              </Field>
              <Field label="Data key reuse period" error={show ? p.reuse : undefined} help={<>How long SQS reuses a data key before calling KMS again. Range <b>1 minute</b> to <b>24 hours</b>, default <b>5 minutes</b>. <span data-testid="reuse-stored">Stored, not applied locally.</span></>} testId="field-reuse">
                {(id) => <TimeInput id={id} value={f.reuse} onChange={(reuse) => set({ reuse })} units={timeUnits("reuse")} label="Data key reuse period" testId="queue-reuse" />}
              </Field>
            </>
          )}
        </>
      )}
      <RuleHelp>Only one server-side encryption option applies to a queue at a time. Turning off KMS encryption does not turn on SSE-SQS by itself.</RuleHelp>
    </FormSection>
  );
}

/** The queue messages that keep failing move to: chosen from the queues of the same type, with the retention it keeps. */
export function DeadLetter({ f, set, p, show, self, region, account, empty }: FieldProps & { self: string; region: string; account: string; empty?: boolean }) {
  const list = useResourceList("sqs", "queues");
  const rows = (list.data?.pages.flatMap((pg) => pg.rows) ?? []).filter((r) => r.name !== self && (r.cells[0] === "FIFO") === f.fifo);
  const arn = (name: string) => `arn:aws:sqs:${region}:${account}:${name}`;
  const name = f.dlq ? f.dlq.split(":").pop()! : "";
  const picked = useResourceDetail("sqs", "queues", name, name !== "");
  const retention = name && picked.data ? queueOf(picked.data).retention : null;
  // The queues of this type that are already somebody's dead-letter queue (the list's Dead-letter queue column names them).
  const targets = new Set(listRows(list.data?.pages.flatMap((pg) => pg.rows) ?? []).map((r) => r.dlq?.split(":").pop()).filter(Boolean));
  const real = rows.filter((r) => targets.has(r.name));
  const sample = useResourceDetail("sqs", "queues", real[0]?.name ?? "", real.length > 0 && !f.dlqOn);
  const keeps = sample.data ? queueOf(sample.data).retention : null;
  const summary = f.dlqOn ? `Enabled: ${name || "no queue chosen"} after ${f.maxReceives || "?"} receives.` : "Disabled.";
  return (
    <FormSection title="Dead-letter queue" optional collapsible defaultOpen={!useContext(FoldedStart)} description="Receives messages that could not be processed from this queue." summary={summary} testId="section-dlq">
      <div className="flex items-center gap-3">
        <Switch id="dlq-on" checked={f.dlqOn} onCheckedChange={(dlqOn) => set({ dlqOn })} data-testid="dlq-on" />
        <Label htmlFor="dlq-on">{f.dlqOn ? "Enabled" : "Disabled"}</Label>
      </div>
      {!f.dlqOn && (
        <RuleHelp>
          {f.fifo ? <>The dead-letter queue of a FIFO queue must also be a FIFO queue. {rows.length === 0 ? "There is no FIFO queue to use yet. Create one first; SQS does not create it for you." : `${rows.map((r) => r.name).join(", ")} ${rows.length === 1 ? "is the only other FIFO queue" : "are the other FIFO queues"} in this Region${real.length === 0 ? ", and none is a dead-letter queue yet, so there is no good target. Create one first; SQS does not create it for you." : "."}`}</> : <>Turn this on to move messages that keep failing to a dead-letter queue. Create the queue first; SQS does not create it, and a standard queue needs a standard dead-letter queue.{real.length > 0 && !empty ? ` ${real.slice(0, 3).map((r) => r.name).join(", ")} ${real.length === 1 ? "is one" : "are some"}${keeps !== null ? `; ${real[0]!.name} keeps messages for ${wait(keeps)}` : ""}.` : ""}</>}
        </RuleHelp>
      )}
      {f.dlqOn && (
        <>
          <Field label="Dead-letter queue" error={show ? p.dlq : undefined} hint={f.dlq ? <><span className="font-mono">{f.dlq}</span>{retention !== null ? <>. {name} retains for {wait(retention)}; keep a dead-letter queue's retention longer than the source's.</> : null}</> : undefined} testId="field-dlq">
            {(id) => <Combobox id={id} options={rows.map((r) => ({ value: arn(r.name), label: r.label }))} value={f.dlq || null} onChange={(dlq) => set({ dlq: dlq ?? "" })} placeholder={list.isPending ? "Reading queues…" : rows.length === 0 ? "There is no queue to choose" : "Choose a queue"} search="Search queues…" empty="No queue matches." className="w-full font-mono" aria-label="Dead-letter queue" />}
          </Field>
          <Field label="Maximum receives" error={show ? p.maxReceives : undefined} help={<>How many times a message can be received before it moves to the dead-letter queue. Range <b>{LIMITS.maxReceives.min} to {LIMITS.maxReceives.max.toLocaleString("en-US")}</b>. A message whose receive count exceeds the value moves.</>} testId="field-max-receives">
            {(id) => <Input id={id} value={f.maxReceives} onChange={(e) => set({ maxReceives: e.target.value })} inputMode="numeric" className="w-32 font-mono" autoComplete="off" data-testid="dlq-max" />}
          </Field>
        </>
      )}
    </FormSection>
  );
}

/** The tags: new ones on a create, or the queue's own on an edit (saved with TagQueue and UntagQueue). */
export function TagsSection({ f, set, show, note }: Pick<FieldProps, "f" | "set" | "show"> & { note?: React.ReactNode }) {
  const n = filled(f.tags).length;
  return (
    <FormSection title="Tags" optional collapsible defaultOpen={!useContext(FoldedStart)} description="Cost allocation tags. Tags are case-sensitive, and a new tag with an existing key overwrites it. Adding more than 50 tags to a queue isn't recommended." summary={n > 0 ? `${n} ${n === 1 ? "tag" : "tags"}.` : "No tags."} testId="section-tags">
      <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={show} testId="form-tags" />
      {note}
    </FormSection>
  );
}

