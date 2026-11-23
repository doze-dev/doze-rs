// Create event notification and Create lifecycle rule: each adds one rule to a configuration S3 replaces whole, so the call
// carries the rules the bucket already has, and the page says so.
import { useState } from "react";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, newRow, pairsOf, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormSection, FunctionPicker, ResourcePicker, TAG_RULES, TopicPicker } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { NONE, useSetting } from "./api";
import { type LifecycleForm, lifecycleProblem, lifecycleRule, lifecycleRules, notificationRules, type Rule, withNotification, withRule } from "./config";
import { RadioCards } from "./controls";
import { Loaded, SettingForm } from "./edit";
import { Mono } from "./parts";
import { fifoDestinationHint } from "./words";

const isFifo = (r: { name: string }) => r.name.endsWith(".fifo");

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
  // A FIFO queue cannot be a destination (the call is refused with InvalidArgument), so the list does not offer one and the hint names them.
  const queues = useResourceList("sqs", "queues");
  const fifoNames = (queues.data?.pages.flatMap((p) => p.rows) ?? []).filter(isFifo).map((r) => r.name);
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
        <Field label={kind === "Lambda Function" ? "Lambda function" : kind === "SQS Queue" ? "SQS queue" : "SNS topic"} help={kind === "SQS Queue" ? `Choose a queue from the list, or choose Enter ARN and type its ARN. Before S3 can publish, you must grant the Amazon S3 principal permission to send to the queue. ${fifoDestinationHint(fifoNames)}` : "Choose from the list, or choose Enter ARN and type the ARN."}>
          {(i) => (typed ? <Input id={i} value={arn} onChange={(e) => setArn(e.target.value)} placeholder="arn:aws:…" className="font-mono" spellCheck={false} data-testid="dest-arn" /> : kind === "Lambda Function" ? <FunctionPicker id={i} by="name" value={picked} onChange={setPicked} /> : kind === "SNS Topic" ? <TopicPicker id={i} by="name" value={picked} onChange={setPicked} /> : <ResourcePicker id={i} service="sqs" kind="queues" noun="queue" by="name" hide={isFifo} value={picked} onChange={setPicked} />)}
        </Field>
        <button type="button" className="w-fit text-[13px] font-medium text-link hover:underline" onClick={() => setTyped(!typed)} data-testid="dest-enter-arn">{typed ? "Choose from the list" : "Enter ARN"}</button>
        {dest && <p className="text-[13px] text-muted-foreground">Destination <Mono>{dest}</Mono></p>}
      </FormSection>
    </SettingForm>
  );
}

const BLANK: LifecycleForm = { name: "", allObjects: false, prefix: "", tags: [], min: "", max: "", expireDays: "", noncurrentDays: "", keepNewer: "", abortDays: "", deleteMarkers: false };

export function LifecycleCreate({ name }: OwnProps) {
  const q = useSetting(name!, "GetBucketLifecycleConfiguration", NONE.lifecycle);
  return <Loaded q={q} what="The lifecycle rules">{(c) => <LifecycleFormPage name={name!} current={c ?? { Rules: [] }} />}</Loaded>;
}

/** A tick with its title and the words under it; what it reveals comes after, indented. */
function Choice({ id, checked, onChange, title, description, disabled = false, children }: { id: string; checked: boolean; onChange?: (on: boolean) => void; title: React.ReactNode; description?: React.ReactNode; disabled?: boolean; children?: React.ReactNode }) {
  return (
    <div className="grid gap-3">
      <div className={`flex items-start gap-3 ${disabled ? "opacity-60" : ""}`}>
        <Checkbox id={id} checked={checked} disabled={disabled} onCheckedChange={(v) => onChange?.(v === true)} className="mt-0.5" data-testid={id} />
        <div className="grid gap-0.5">
          <Label htmlFor={id} className="font-medium text-bright">{title}</Label>
          {description && <p className="text-[13px] leading-5 text-muted-foreground">{description}</p>}
        </div>
      </div>
      {checked && children && <div className="ml-7 grid gap-4">{children}</div>}
    </div>
  );
}

function LifecycleFormPage({ name, current }: { name: string; current: unknown }) {
  const [f, setF] = useState<LifecycleForm>(BLANK);
  const [tags, setTags] = useState<KeyValueRow[]>(() => [newRow()]);
  const [ack, setAck] = useState(false);
  // What is ticked: a value left in a field that is not ticked is not sent.
  const [on, setOn] = useState({ min: false, max: false, expire: false, previous: false, cleanup: false });
  const tick = (p: Partial<typeof on>) => setOn({ ...on, ...p });
  const set = (p: Partial<LifecycleForm>) => setF({ ...f, ...p });
  const sent: LifecycleForm = { ...f, tags: pairsOf(filled(tags)), min: on.min ? f.min : "", max: on.max ? f.max : "", expireDays: on.expire ? f.expireDays : "", noncurrentDays: on.previous ? f.noncurrentDays : "", keepNewer: on.previous ? f.keepNewer : "", abortDays: on.cleanup ? f.abortDays : "", deleteMarkers: on.cleanup && f.deleteMarkers };
  const unfilled = (on.min && !f.min ? "Give the minimum object size in bytes." : null) ?? (on.max && !f.max ? "Give the maximum object size in bytes." : null) ?? (on.expire && !f.expireDays ? "Give the days after creation at which current versions expire." : null) ?? (on.previous && !f.noncurrentDays ? "Give the days after which a noncurrent version is deleted." : null) ?? (on.cleanup && !f.abortDays && !f.deleteMarkers ? "Choose what to clean up: expired delete markers, or incomplete multipart uploads." : null);
  const problem = unfilled ?? lifecycleProblem(sent, lifecycleRules(current).map((r) => r.name)) ?? (!f.allObjects && !rowsValid(tags, TAG_RULES) ? "A tag key must be filled in and unique." : null) ?? (f.allObjects && !ack ? "Acknowledge that the rule applies to all objects in the bucket." : null);
  const day = (key: keyof LifecycleForm, label: string, help?: string) => (
    <Field label={label} help={help}>{(i) => <Input id={i} value={String(f[key])} onChange={(e) => set({ [key]: e.target.value })} inputMode="numeric" className="w-40 font-mono" data-testid={`lc-${key}`} />}</Field>
  );
  const existing = lifecycleRules(current);
  return (
    <SettingForm name={name} tab="management" title="Create lifecycle rule" sub={`Tell S3 what to do with objects in ${name} as they age.`} operation="PutBucketLifecycleConfiguration" input={problem ? null : { LifecycleConfiguration: withRule(current, lifecycleRule(sent)) }} saved={`Lifecycle rule ${f.name} created.`} submitLabel="Create rule" disabled={Boolean(problem)} disabledWhy={problem}
      note="Adding a rule replaces the bucket's lifecycle configuration, so the call carries every rule: the ones that exist and the new one.">
      <FormSection title="Lifecycle rule configuration">
        <Field label="Lifecycle rule name" help={<>Up to <b>255</b> characters, unique within the bucket.{existing.length > 0 && <> {existing.map((r) => r.name).join(", ")} {existing.length === 1 ? "is the rule" : "are the rules"} already here.</>}</>}>
          {(i) => <Input id={i} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="prune-old-versions" className="font-mono" data-testid="lc-name" />}
        </Field>
        <div className="grid gap-2"><span className="text-[14px] font-medium text-bright">Choose the scope of the lifecycle rule</span>
          <RadioCards name="lc-scope" value={f.allObjects ? "all" : "some"} onChange={(v) => set({ allObjects: v === "all" })} options={[{ value: "some", title: "Limit the scope to specific prefixes or tags", description: "The local lifecycle janitor applies the prefix, tag and size filters it is given, and only to what they select." }, { value: "all", title: "This rule applies to all objects in the bucket", description: "You must also acknowledge that the rule applies to all objects in the bucket." }]} />
        </div>
        {f.allObjects && <div className="flex items-center gap-2"><Checkbox id="lc-ack" checked={ack} onCheckedChange={(v) => setAck(v === true)} data-testid="lc-ack" /><Label htmlFor="lc-ack" className="font-normal">I acknowledge that this rule will apply to all objects in the bucket.</Label></div>}
        {!f.allObjects && (
          <>
            <Field label="Prefix (optional)" help="The rule applies to keys that begin with this prefix.">{(i) => <Input id={i} value={f.prefix} onChange={(e) => set({ prefix: e.target.value })} placeholder="catalog/" className="font-mono" data-testid="lc-prefix" />}</Field>
            <Field label="Tags (optional)" help="The rule applies to objects that have every one of these tags.">{() => <KeyValueEditor rows={tags} onChange={setTags} rules={TAG_RULES} addLabel="Add tag" testId="lc-tags" />}</Field>
            <div className="grid gap-3"><span className="text-[14px] font-medium text-bright">Object size<span className="ml-2 font-normal text-muted-foreground">Optional</span></span>
              <Choice id="lc-min-on" checked={on.min} onChange={(v) => tick({ min: v })} title="Specify minimum object size">{day("min", "Minimum object size in bytes")}</Choice>
              <Choice id="lc-max-on" checked={on.max} onChange={(v) => tick({ max: v })} title="Specify maximum object size">{day("max", "Maximum object size in bytes")}</Choice>
            </div>
          </>
        )}
      </FormSection>
      <FormSection title="Lifecycle rule actions">
        <Choice id="lc-transition-current" checked={false} disabled title="Transition current versions of objects between storage classes" description="Not offered here: there is one storage class locally, so a transition has nowhere to go." />
        <Choice id="lc-transition-previous" checked={false} disabled title="Transition previous versions of objects between storage classes" description="Not offered here: there is one storage class locally, so a transition has nowhere to go. A transition put with the AWS CLI is kept and returned, and never acted on." />
        <Choice id="lc-expire-on" checked={on.expire} onChange={(v) => tick({ expire: v })} title="Expire current versions of objects" description="On a versioned bucket this adds a delete marker.">{day("expireDays", "Days after object creation")}</Choice>
        <Choice id="lc-previous-on" checked={on.previous} onChange={(v) => tick({ previous: v })} title="Permanently delete previous versions of objects">
          {day("noncurrentDays", "Days after objects become noncurrent", "The value must be a non-zero positive integer: how many days a version stays noncurrent before S3 can delete it. Permanent deletion cannot be undone.")}
          {day("keepNewer", "Number of newer versions to retain (optional)", "The local janitor keeps this many of the newest noncurrent versions whatever their age. On AWS you can retain up to 100 newer noncurrent versions.")}
        </Choice>
        <Choice id="lc-cleanup-on" checked={on.cleanup} onChange={(v) => tick({ cleanup: v })} title="Delete expired delete markers or incomplete multipart uploads" description="Incomplete multipart uploads are aborted after the number of days you set; the janitor also removes expired object delete markers.">
          <div className="flex items-center gap-2"><Checkbox id="lc-markers" checked={f.deleteMarkers} onCheckedChange={(v) => set({ deleteMarkers: v === true })} data-testid="lc-deleteMarkers" /><Label htmlFor="lc-markers" className="font-normal">Delete expired object delete markers</Label></div>
          {day("abortDays", "Delete incomplete multipart uploads: days after they start")}
        </Choice>
        <p className="text-[13px] text-muted-foreground">There is a delay between a rule being met and the version being removed, because S3 queues expirations asynchronously. Here rules take effect at the next UTC midnight.</p>
      </FormSection>
    </SettingForm>
  );
}
