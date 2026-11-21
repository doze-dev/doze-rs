// Create event notification and Create lifecycle rule: each adds one rule to a configuration S3 replaces whole, so the call
// carries the rules the bucket already has, and the page says so.
import { useState } from "react";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useAbout } from "../../../api/queries";
import { Field, FormSection, FunctionPicker, LocalStatus, ResourcePicker, TopicPicker } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { NONE, useSetting } from "./api";
import { type LifecycleForm, lifecycleProblem, lifecycleRule, lifecycleRules, notificationRules, type Rule, withNotification, withRule } from "./config";
import { RadioCards } from "./controls";
import { Loaded, SettingForm } from "./edit";
import { Mono } from "./parts";

const EVENTS: [string, string][] = [["s3:ObjectCreated:*", "Notify whenever an object is created, regardless of the API that was used to create it."], ["s3:ObjectRemoved:*", "Notify anytime an object is deleted."]];
type Kind = Rule["kind"];

export function NotificationCreate({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketNotificationConfiguration", []);
  return <Loaded q={q} what="The notification rules">{(c) => <NotificationForm name={name!} current={c ?? {}} />}</Loaded>;
}
function NotificationForm({ name, current }: { name: string; current: unknown }) {
  const about = useAbout();
  const [id, setId] = useState("");
  const [prefix, setPrefix] = useState("");
  const [suffix, setSuffix] = useState("");
  const [events, setEvents] = useState<string[]>(["s3:ObjectCreated:*"]);
  const [kind, setKind] = useState<Kind>("Lambda Function");
  const [picked, setPicked] = useState<string | null>(null);
  const [arn, setArn] = useState("");
  const [typed, setTyped] = useState(false);
  const existing = notificationRules(current);
  const region = about.data?.region ?? "us-east-1";
  const account = about.data?.account ?? "";
  const from = (n: string) => `arn:aws:${kind === "Lambda Function" ? "lambda" : kind === "SQS Queue" ? "sqs" : "sns"}:${region}:${account}:${kind === "Lambda Function" ? "function:" : ""}${n}`;
  const dest = typed ? arn : picked ? from(picked) : "";
  const input = events.length > 0 && dest ? { NotificationConfiguration: withNotification(current, { id, events, prefix, suffix, kind, arn: dest }) } : null;
  const toggle = (e: string, on: boolean) => setEvents(on ? [...events, e] : events.filter((x) => x !== e));
  return (
    <SettingForm name={name} tab="properties" title="Create event notification" sub={`Send a message when something happens to an object in ${name}.`} operation="PutBucketNotificationConfiguration" input={input} saved={`Event notification ${id || "rule"} created.`} disabled={!input} disabledWhy="Choose the events and a destination."
      note="The call replaces the whole notification configuration, so it carries the rule that is already there. doze-aws accepts a Lambda or SNS destination that does not exist yet; an SQS queue must exist and must not be FIFO.">
      <FormSection title="General configuration">
        <Field label="Event name (optional)" help={existing.length > 0 ? <>If you leave the name empty, S3 generates a globally unique identifier (GUID) and uses it. The existing {existing.length === 1 ? "rule is" : "rules are"} called <b>{existing.map((r) => r.id).join(", ")}</b>.</> : "If you leave the name empty, S3 generates a globally unique identifier (GUID) and uses it."}>
          {(i) => <Input id={i} value={id} onChange={(e) => setId(e.target.value)} placeholder="image-removed" className="font-mono" data-testid="notification-name" />}
        </Field>
        <Field label="Prefix (optional)" help="Limit notifications to keys that begin with a prefix such as images/.">{(i) => <Input id={i} value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="images/" className="font-mono" />}</Field>
        <Field label="Suffix (optional)" help="Limit notifications to keys that end with these characters.">{(i) => <Input id={i} value={suffix} onChange={(e) => setSuffix(e.target.value)} placeholder=".jpg" className="font-mono" />}</Field>
      </FormSection>
      <FormSection title="Event types" description="Each wildcard covers the finer events beneath it: for create, Put, Copy and CompleteMultipartUpload; for delete, Delete. These notifications do not fire for automatic deletes from lifecycle configurations.">
        {EVENTS.map(([e, d]) => (
          <div key={e} className="flex items-start gap-3">
            <Checkbox id={e} checked={events.includes(e)} onCheckedChange={(v) => toggle(e, v === true)} className="mt-0.5" data-testid={`event-${e.split(":")[1]}`} />
            <div className="grid gap-0.5"><Label htmlFor={e} className="font-mono text-[13px] text-bright">{e}</Label><p className="text-[13px] text-muted-foreground">{d}</p></div>
          </div>
        ))}
      </FormSection>
      <FormSection title="Destination">
        <RadioCards name="dest-kind" value={kind} onChange={(k) => (setKind(k), setPicked(null))} options={(["Lambda Function", "SNS Topic", "SQS Queue"] as Kind[]).map((k) => ({ value: k, title: k }))} className="sm:grid-cols-3" />
        <Field label={kind === "Lambda Function" ? "Lambda function" : kind === "SQS Queue" ? "SQS queue" : "SNS topic"} help={kind === "SQS Queue" ? "Choose a queue from the list, or choose Enter ARN and type its ARN. Before S3 can publish, you must grant the Amazon S3 principal permission to send to the queue. FIFO queues are not supported as a destination: naming one is refused with InvalidArgument." : "Choose from the list, or choose Enter ARN and type the ARN."}>
          {(i) => (typed ? <Input id={i} value={arn} onChange={(e) => setArn(e.target.value)} placeholder="arn:aws:…" className="font-mono" spellCheck={false} data-testid="dest-arn" /> : kind === "Lambda Function" ? <FunctionPicker id={i} by="name" value={picked} onChange={setPicked} /> : kind === "SNS Topic" ? <TopicPicker id={i} by="name" value={picked} onChange={setPicked} /> : <ResourcePicker id={i} service="sqs" kind="queues" noun="queue" by="name" value={picked} onChange={setPicked} />)}
        </Field>
        <button type="button" className="w-fit text-[13px] font-medium text-link hover:underline" onClick={() => setTyped(!typed)} data-testid="dest-enter-arn">{typed ? "Choose from the list" : "Enter ARN"}</button>
        {dest && <p className="text-[13px] text-muted-foreground">Destination <Mono>{dest}</Mono></p>}
      </FormSection>
    </SettingForm>
  );
}

const BLANK: LifecycleForm = { name: "", allObjects: false, prefix: "", min: "", max: "", expireDays: "", noncurrentDays: "", keepNewer: "", abortDays: "", deleteMarkers: false };

export function LifecycleCreate({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketLifecycleConfiguration", NONE.lifecycle);
  return <Loaded q={q} what="The lifecycle rules">{(c) => <LifecycleFormPage name={name!} current={c ?? { Rules: [] }} />}</Loaded>;
}
function LifecycleFormPage({ name, current }: { name: string; current: unknown }) {
  const [f, setF] = useState<LifecycleForm>(BLANK);
  const [ack, setAck] = useState(false);
  const set = (p: Partial<LifecycleForm>) => setF({ ...f, ...p });
  const problem = lifecycleProblem(f, lifecycleRules(current).map((r) => r.name)) ?? (f.allObjects && !ack ? "Acknowledge that the rule applies to all objects in the bucket." : null);
  const day = (key: keyof LifecycleForm, label: string, help?: string) => (
    <Field label={label} help={help}>{(i) => <Input id={i} value={String(f[key])} onChange={(e) => set({ [key]: e.target.value })} inputMode="numeric" className="w-40 font-mono" data-testid={`lc-${key}`} />}</Field>
  );
  return (
    <SettingForm name={name} tab="management" title="Create lifecycle rule" sub={`Tell S3 what to do with objects in ${name} as they age.`} operation="PutBucketLifecycleConfiguration" input={problem ? null : { LifecycleConfiguration: withRule(current, lifecycleRule(f)) }} saved={`Lifecycle rule ${f.name} created.`} submitLabel="Create rule" disabled={Boolean(problem)} disabledWhy={problem}
      note="Adding a rule replaces the bucket's lifecycle configuration, so the call carries every rule: the ones that exist and the new one.">
      <FormSection title="Lifecycle rule configuration">
        <Field label="Lifecycle rule name" help={<>Up to 255 characters, unique within the bucket.{lifecycleRules(current).length > 0 && <> {lifecycleRules(current).map((r) => r.name).join(", ")} {lifecycleRules(current).length === 1 ? "is the rule" : "are the rules"} already here.</>}</>}>
          {(i) => <Input id={i} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="prune-old-versions" className="font-mono" data-testid="lc-name" />}
        </Field>
        <RadioCards name="lc-scope" value={f.allObjects ? "all" : "some"} onChange={(v) => set({ allObjects: v === "all" })} options={[{ value: "some", title: "Limit the scope to specific prefixes or object sizes", description: "The local lifecycle janitor applies the prefix, tag and size filters it is given, and only to what they select." }, { value: "all", title: "This rule applies to all objects in the bucket", description: "You must also acknowledge that the rule applies to all objects in the bucket." }]} />
        {f.allObjects && <div className="flex items-center gap-2"><Checkbox id="lc-ack" checked={ack} onCheckedChange={(v) => setAck(v === true)} data-testid="lc-ack" /><Label htmlFor="lc-ack" className="font-normal">I acknowledge that this rule will apply to all objects in the bucket.</Label></div>}
        {!f.allObjects && (
          <>
            <Field label="Prefix (optional)" help="The rule applies to keys that begin with this prefix.">{(i) => <Input id={i} value={f.prefix} onChange={(e) => set({ prefix: e.target.value })} placeholder="catalog/" className="font-mono" data-testid="lc-prefix" />}</Field>
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Minimum object size in bytes (optional)">{(i) => <Input id={i} value={f.min} onChange={(e) => set({ min: e.target.value })} inputMode="numeric" className="font-mono" />}</Field>
              <Field label="Maximum object size in bytes (optional)">{(i) => <Input id={i} value={f.max} onChange={(e) => set({ max: e.target.value })} inputMode="numeric" className="font-mono" />}</Field>
            </div>
          </>
        )}
      </FormSection>
      <FormSection title="Lifecycle rule actions" description="Choose what the rule does. Moving objects between storage classes is not offered: there is one storage class locally, so a transition would have nowhere to go.">
        {day("expireDays", "Expire current versions: days after creation", "On a versioned bucket this adds a delete marker.")}
        {day("noncurrentDays", "Permanently delete noncurrent versions: days after objects become noncurrent", "The value must be a non-zero positive integer: how many days a version stays noncurrent before S3 can delete it. Permanent deletion cannot be undone.")}
        {day("keepNewer", "Number of newer versions to retain (optional)", "The local janitor keeps this many of the newest noncurrent versions whatever their age.")}
        {day("abortDays", "Delete incomplete multipart uploads: days after they start")}
        <div className="flex items-center gap-2"><Checkbox id="lc-markers" checked={f.deleteMarkers} onCheckedChange={(v) => set({ deleteMarkers: v === true })} data-testid="lc-deleteMarkers" /><Label htmlFor="lc-markers" className="font-normal">Delete expired object delete markers</Label></div>
        <p className="text-[13px] text-muted-foreground">There is a delay between a rule being met and the version being removed, because S3 queues expirations asynchronously. Here rules take effect at the next UTC midnight.</p>
        <LocalStatus state="stored" reason="Transitions between storage classes put with the AWS CLI are kept and returned, and never acted on." />
      </FormSection>
    </SettingForm>
  );
}
