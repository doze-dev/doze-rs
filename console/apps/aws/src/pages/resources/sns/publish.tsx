// Publish message (sns__publish-message, publish-fifo, publish-fifo-refused; topic-published is where it lands): the message, its subject, group,
// deduplication ID and attributes, the call it will make beside it, who gets what, and (for a subscription with a filter policy) whether the message
// passes. The service's refusal is said on the field and in the call.
import { useMemo, useState } from "react";
import { PlusIcon, XIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import { type Attribute, matches, policyWords } from "./filter-match";
import { Rich } from "./parts";
import { attributesOf, bytes, emptyPublish, givenText, messageOf, newAttr, problems, type AttrType, type PublishForm, publishInput } from "./publish-model";
import { remember } from "./publish-result";
import { type Part, b } from "./sentences";
import { useSubscriptions } from "./sub-reads";
import { endpointName } from "./sub-model";
import type { TopicProps } from "./topic-props";
import { count } from "./words";

const REFUSAL_DEDUP = /MessageDeduplicationId/;

export function Publish({ d, t, subs, back, done }: TopicProps) {
  const target = { service: d.service, kind: d.kind, name: d.name };
  const [f, setF] = useState<PublishForm>(emptyPublish);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<PublishForm>) => setF((x) => ({ ...x, ...patch }));
  const send = useOp<{ MessageId: string; SequenceNumber?: string }>(target, "Publish", { keep: true });
  const read = useSubscriptions(subs);
  const confirmed = subs.filter((s) => !s.pending);
  const p = problems(f, t);
  const input = useMemo(() => ({ TopicArn: t.arn, ...publishInput(f) }), [f, t.arn]);
  const refused = send.error instanceof ApiFailure ? { code: send.error.code, message: send.error.message } : send.error ? { code: "Error", message: (send.error as Error).message } : null;
  const dedupRefused = refused && REFUSAL_DEDUP.test(refused.message);
  const attrs = attributesOf(f);
  const filters = read.ready ? confirmed.map((s, i) => ({ s, d: read.subs[i]! })).filter((x) => x.d.filter) : [];
  const submit = () => {
    setTried(true);
    if (Object.keys(p).length > 0) return;
    send.mutate(publishInput(f), {
      onSuccess: (r) => {
        remember(d.name, { messageId: r.MessageId, sequenceNumber: r.SequenceNumber ?? null, group: f.group.trim() || null, dedup: f.dedup.trim() || null, attributes: attrs, custom: f.structure === "custom", at: Date.now() });
        done("published");
      },
    });
  };
  const sentence = sentenceOf(f, confirmed.length, filters.map((x) => ({ name: endpointName(x.s.protocol, x.s.endpoint), verdict: x.d.scope === "MessageBody" || f.structure === "custom" ? ("unknown" as const) : matches(x.d.filter!, attrs) })), attrs);
  const crumbs = [{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }];
  return (
    <FormPage
      title="Publish message to topic"
      sub={<span className="font-mono">{d.name}</span>}
      crumbs={crumbs}
      request={{ service: "sns", operation: "Publish", input }}
      refusal={refused}
      callNote={refused && dedupRefused ? <>A missing MessageGroupId fails too: every message on a FIFO topic must include one. With ContentBasedDeduplication on, SNS makes the deduplication ID from a SHA-256 hash of the body (attributes excluded) and the field becomes optional.</> : t.fifo ? "If an ID is missing, Publish fails." : "A successful Publish returns a MessageId, and SNS then delivers to the subscribers straight away."}
      banner={
        <div className="grid gap-4">
          {refused ? <Alert tone="bad" title="Nothing was published." testId="publish-refused">Publish failed with an error: {refused.code}: {refused.message}. No MessageId came back.</Alert> : <Rich testId="publish-sentence" parts={sentence} />}
        </div>
      }
      aside={<Aside f={f} filters={filters.map((x) => ({ name: endpointName(x.s.protocol, x.s.endpoint), policy: x.d.filter!, scope: x.d.scope, verdict: x.d.scope === "MessageBody" || f.structure === "custom" ? ("unknown" as const) : matches(x.d.filter!, attrs) }))} who={confirmed.map((s, i) => ({ name: endpointName(s.protocol, s.endpoint), protocol: s.protocol, raw: read.ready ? read.subs[i]!.raw : null }))} />}
      submitLabel="Publish message"
      pending={send.isPending}
      disabled={tried && Object.keys(p).length > 0}
      disabledWhy={Object.values(p)[0]}
      onCancel={back}
      onSubmit={submit}
      testId="publish-form"
    >
      <FormSection title="Message details" testId="section-details">
        <Field label="Topic ARN" testId="field-arn">{(id) => <Input id={id} value={t.arn} readOnly className="bg-muted font-mono text-[13px] text-muted-foreground" data-testid="publish-arn" />}</Field>
        <Field label="Subject" optional error={tried ? p.subject : undefined} help="Included, if present, in the standard JSON messages delivered to endpoints. Less than 100 characters, with no line breaks or control characters." testId="field-subject">
          {(id) => <Input id={id} value={f.subject} onChange={(e) => set({ subject: e.target.value })} placeholder="Order 10482 placed" className="bg-card" autoComplete="off" data-testid="publish-subject" />}
        </Field>
        <Field label="Message group ID" optional={!t.fifo} error={tried ? p.group : undefined} help={t.fifo ? "Required on a FIFO topic. Up to 128 letters, numbers and punctuation." : "On a standard topic the group ID goes only to Amazon SQS standard subscriptions, where it turns on fair queues. Every other endpoint type ignores it."} hint={t.fifo ? <><b className="font-semibold text-foreground">Messages in the same group are delivered in the order they are published;</b> different groups can interleave. Grouping by order keeps one order's events in sequence.</> : undefined} testId="field-group">
          {(id) => <Input id={id} value={f.group} onChange={(e) => set({ group: e.target.value })} placeholder="Enter a message group ID" className="bg-card font-mono" autoComplete="off" data-testid="publish-group" />}
        </Field>
        {t.fifo && (
          <Field label="Message deduplication ID" optional={t.dedup} error={tried ? p.dedup : undefined} refusal={dedupRefused ? { why: "Required: this topic does not have ContentBasedDeduplication set." } : null} help="Up to 128 letters, numbers and punctuation. Optional only when the topic has content-based deduplication on." hint={<><b className="font-semibold text-foreground">This topic has content-based deduplication {t.dedup ? "on, so the ID is optional" : "off, so the ID is required"}.</b> A second message with the same ID inside the 5 minute deduplication interval is accepted but not delivered. Scope: {t.scope === "MessageGroup" ? "each message group" : "the entire topic"}.</>} testId="field-dedup">
            {(id) => <Input id={id} value={f.dedup} onChange={(e) => set({ dedup: e.target.value })} placeholder="Enter a message deduplication ID" className="bg-card font-mono" autoComplete="off" data-testid="publish-dedup" />}
          </Field>
        )}
      </FormSection>
      <FormSection title="Message body" testId="section-body">
        <RadioGroup value={f.structure} onValueChange={(v) => set({ structure: v as PublishForm["structure"] })} className="grid gap-3 sm:grid-cols-2" aria-label="Message structure">
          {([["identical", "Identical payload for all delivery protocols", "One message, sent to every subscription as is."], ["custom", "Custom payload for each delivery protocol", "A JSON object with a different message for each protocol."]] as const).map(([v, title, text]) => (
            <Label key={v} htmlFor={`structure-${v}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
              <RadioGroupItem id={`structure-${v}`} value={v} className="mt-0.5" data-testid={`structure-${v}`} />
              <span className="grid gap-0.5"><span className="font-medium text-bright">{title}</span><span className="text-[13px] text-muted-foreground">{text}</span></span>
            </Label>
          ))}
        </RadioGroup>
        <Field label="Message" error={tried ? (f.structure === "custom" ? p.custom : p.body) ?? p.size : p.size} hint={f.structure === "custom" ? undefined : undefined} help={f.structure === "custom" ? <><b className="font-semibold text-foreground">The JSON needs a top-level default key whose value is a string.</b> Other keys name protocols (sqs, http, https, lambda). Values must be simple strings, which is why the sqs value is JSON written as text. Non-string values make that key ignored; duplicate keys are refused, and one bad key refuses the whole publish.</> : undefined} testId="field-message">
          {(id) => (
            <>
              <Textarea id={id} value={f.structure === "custom" ? f.custom : f.body} onChange={(e) => set(f.structure === "custom" ? { custom: e.target.value } : { body: e.target.value })} rows={6} spellCheck={false} className="min-h-32 bg-card font-mono text-[13px] leading-5" data-testid="publish-message" />
              <p className="mt-1.5 text-[13px] text-muted-foreground" data-testid="publish-size">{bytes(messageOf(f)).toLocaleString("en-US")} bytes of the {t.maxSize.toLocaleString("en-US")}-byte limit{t.maxSizeSet ? "" : " (the topic keeps the 256 KiB default)"}.</p>
            </>
          )}
        </Field>
      </FormSection>
      <FormSection title="Message attributes" description="Optional metadata that subscription filter policies can match." testId="section-attributes">
        {f.structure === "custom" ? (
          <RuleHelp><b>Message attributes are sent only when the message structure is String, not JSON.</b> With a custom payload they would be dropped, so this publish carries none.</RuleHelp>
        ) : (
          <>
            {f.attrs.map((a) => (
              <div key={a.id} className="grid items-end gap-3 sm:grid-cols-[11rem_minmax(0,1fr)_minmax(0,1fr)_auto]" data-testid="attr-row">
                <div className="grid gap-1.5"><Label>Type</Label>
                  <Select value={a.type} onValueChange={(type) => set({ attrs: f.attrs.map((x) => (x.id === a.id ? { ...x, type: type as AttrType } : x)) })}>
                    <SelectTrigger className="w-full bg-card" aria-label="Attribute type" data-testid="attr-type"><SelectValue /></SelectTrigger>
                    <SelectContent>{(["String", "String.Array", "Number", "Binary"] as const).map((v) => <SelectItem key={v} value={v}>{v}</SelectItem>)}</SelectContent>
                  </Select>
                </div>
                <div className="grid gap-1.5"><Label>Name</Label><Input value={a.name} onChange={(e) => set({ attrs: f.attrs.map((x) => (x.id === a.id ? { ...x, name: e.target.value } : x)) })} className="bg-card font-mono" autoComplete="off" data-testid="attr-name" /></div>
                <div className="grid gap-1.5"><Label>Value</Label><Input value={a.value} onChange={(e) => set({ attrs: f.attrs.map((x) => (x.id === a.id ? { ...x, value: e.target.value } : x)) })} className="bg-card font-mono" autoComplete="off" data-testid="attr-value" /></div>
                <Button type="button" variant="outline" size="icon" className="bg-card" aria-label="Remove attribute" onClick={() => set({ attrs: f.attrs.filter((x) => x.id !== a.id) })}><XIcon /></Button>
              </div>
            ))}
            <div><Button type="button" variant="outline" className="bg-card" onClick={() => set({ attrs: [...f.attrs, newAttr()] })} data-testid="attr-add"><PlusIcon /> Add attribute</Button></div>
            <RuleHelp><b>String, String.Array and Number attributes are matched against the filter policy</b> when its scope is MessageAttributes, the default. Binary attributes are ignored. Types: String, String.Array, Number, Binary. A name uses A-Z, a-z, 0-9, underscore, hyphen and period, is at most 256 characters, and cannot start with AWS. or Amazon. Attributes count toward the message size limit.</RuleHelp>
          </>
        )}
      </FormSection>
    </FormPage>
  );
}

type Filter = { name: string; verdict: "match" | "no-match" | "unknown" };

function sentenceOf(f: PublishForm, n: number, filters: Filter[], attrs: Record<string, Attribute>): Part[] {
  const topicSubs = n === 0 ? "no confirmed subscription" : count(n, "confirmed subscription");
  if (f.group.trim()) {
    const carries = Object.entries(attrs).map(([k, v]) => `${k} = ${v.value}`);
    const verdict = filters.length === 1 ? (filters[0]!.verdict === "match" ? ", which its filter policy accepts" : filters[0]!.verdict === "no-match" ? ", which its filter policy does not accept" : "") : "";
    return ["This sends ", b("one message"), " in group ", b(f.group.trim()), `. The topic has ${topicSubs}`, ...(carries.length > 0 ? [" and the message carries ", b(carries.join(", "))] : []), `${verdict}.`];
  }
  if (n === 0) return ["This sends ", b("one message"), " to a topic with no confirmed subscription, so it is delivered nowhere."];
  const custom = f.structure === "custom" ? ` Each SQS queue is given the text of the sqs key; a protocol with no key of its own would get default.` : "";
  return ["This sends ", b("one message"), ` to ${topicSubs}.${f.structure === "custom" ? custom : ""}`];
}

/** Under the call: who gets what, and for each filtered subscription whether this message passes. */
function Aside({ f, filters, who }: { f: PublishForm; filters: { name: string; policy: string; scope: string; verdict: "match" | "no-match" | "unknown" }[]; who: { name: string; protocol: string; raw: boolean | null }[] }) {
  return (
    <>
      {filters.length > 0 && (
        <section className="rounded-[14px] border bg-card p-5" data-testid="filter-preview">
          <h2 className="text-[17px] font-semibold tracking-[-0.01em] text-bright">Filter on the {filters.length === 1 ? "one subscription" : `${filters.length} subscriptions`}</h2>
          {filters.map((x) => (
            <div key={x.name} className="mt-3 grid gap-3 text-[13px]">
              {filters.length > 1 && <b className="font-mono text-bright">{x.name}</b>}
              <div><span className="text-muted-foreground">Policy</span><p className="mt-0.5 font-mono text-[13px] text-bright">{policyWords(x.policy)}</p></div>
              <div><span className="text-muted-foreground">This message</span><p className="mt-0.5 font-mono text-[13px] text-bright">{Object.entries(attributesOf(f)).map(([k, v]) => `${k} = ${v.value}`).join(", ") || "no attributes"}</p></div>
              <p className={x.verdict === "match" ? "font-medium text-good" : x.verdict === "no-match" ? "font-medium text-warn" : "text-muted-foreground"} data-testid="filter-verdict"><span aria-hidden className="mr-2 inline-block size-2 rounded-full bg-current" />{x.verdict === "match" ? "Matches, delivered" : x.verdict === "no-match" ? "Does not match, not delivered" : x.scope === "MessageBody" ? "Filters the message body: the service decides" : "More than this page reads: the service decides"}</p>
            </div>
          ))}
        </section>
      )}
      {who.length > 0 && (
        <section className="rounded-[14px] border bg-card p-5" data-testid="who-gets-what">
          <h2 className="text-[17px] font-semibold tracking-[-0.01em] text-bright">Who gets what</h2>
          {who.every((w) => w.raw === false) && <p className="mt-2 text-[13px] leading-5 text-muted-foreground">Raw delivery is off on {who.length === 1 ? "it" : `all ${who.length}`}, so each endpoint gets the JSON envelope with this text in <span className="font-mono">Message</span>.</p>}
          <ul className="mt-3 grid gap-1.5 text-[13px]">
            {who.map((w) => <li key={w.name} className="flex flex-wrap gap-x-3"><span className="font-mono text-bright">{w.name}</span><span className="text-muted-foreground">{w.raw === true ? "the bare message body" : givenText(f, w.protocol)}</span></li>)}
          </ul>
        </section>
      )}
    </>
  );
}
