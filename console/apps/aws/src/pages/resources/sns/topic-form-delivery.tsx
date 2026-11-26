// The later sections of Create topic and Edit topic: the delivery retry policy, delivery status logging, active tracing and tags.
import { useEffect } from "react";
import { Combobox } from "@doze/ui/components/app/combobox";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled } from "@doze/ui/components/app/key-values";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Badge } from "@doze/ui/components/ui/badge";
import { useResourceList } from "../../../api/resources";
import { Field, FormSection, LocalStatus, RuleHelp, TAG_RULES } from "../../../kit";
import { PAGES } from "../../../kit/pickers";
import type { FieldProps } from "./topic-form-fields";
import { LOGGED, type Logged } from "./topic-model";
import type { Retry } from "./topic-form-model";

type RetryField = { key: keyof Retry; unit: string; help: string };

const RETRY: RetryField[] = [
  { key: "numRetries", unit: "retries", help: "The total number of retries, including immediate, pre-backoff, backoff, and post-backoff retries. 0 to 100, default 3." },
  { key: "numNoDelayRetries", unit: "retries", help: "The number of retries to be done immediately, with no delay between them. 0 or greater, default 0." },
  { key: "minDelayTarget", unit: "seconds", help: "The minimum delay for a retry. 1 to the maximum delay, default 20." },
  { key: "numMinDelayRetries", unit: "retries", help: "The number of retries in the pre-backoff phase, with the minimum delay between them. 0 or greater, default 0." },
  { key: "maxDelayTarget", unit: "seconds", help: "The maximum delay for a retry. The minimum delay to 3,600, default 20." },
  { key: "numMaxDelayRetries", unit: "retries", help: "The number of retries in the post-backoff phase, with the maximum delay between them. 0 or greater, default 0." },
];

/** How SNS retries a failed delivery to an HTTP or HTTPS endpoint. doze-aws applies it to those (crates/aws-sns/src/webhook.rs). */
export function RetryPolicy({ f, set, p, show, collapsed, note }: FieldProps & { collapsed: boolean; note?: React.ReactNode }) {
  const r = f.retry;
  const put = (patch: Partial<Retry>) => set({ retry: { ...r, ...patch } });
  return (
    <FormSection title="Delivery retry policy (HTTP/S)" collapsible={collapsed} defaultOpen={!collapsed} description={<>How SNS retries failed deliveries to HTTP/S endpoints. Attribute: <span className="font-mono">DeliveryPolicy</span>.</>} summary="The defaults: 3 retries, 20 seconds apart." testId="section-retry">
      {note && <p className="text-[13.5px] text-muted-foreground" data-testid="retry-note">{note}</p>}
      <div className="grid gap-x-8 gap-y-6 md:grid-cols-2">
        {RETRY.map((x) => (
          <Field key={x.key} label={x.key} help={x.help} testId={`field-${x.key}`}>
            {(id) => (
              <div className="flex items-center gap-2">
                <Input id={id} value={r[x.key]} onChange={(e) => put({ [x.key]: e.target.value })} inputMode="numeric" className="bg-card font-mono" autoComplete="off" data-testid={`retry-${x.key}`} />
                <span className="w-14 text-[13px] text-muted-foreground">{x.unit}</span>
              </div>
            )}
          </Field>
        ))}
        <Field label="backoffFunction" help="The model for backoff between retries: arithmetic, exponential, geometric or linear. Default linear." testId="field-backoffFunction">
          {(id) => (
            <Select value={r.backoffFunction} onValueChange={(backoffFunction) => put({ backoffFunction })}>
              <SelectTrigger id={id} className="w-full bg-card font-mono text-[13px]" data-testid="retry-backoffFunction"><SelectValue /></SelectTrigger>
              <SelectContent>{["arithmetic", "exponential", "geometric", "linear"].map((v) => <SelectItem key={v} value={v}>{v}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <Field label="maxReceivesPerSecond" optional help="The maximum average number of message deliveries per second, per subscription. 1 or greater; default no throttling. Stored, not applied: doze-aws does not throttle." testId="field-maxReceives">
          {(id) => (
            <div className="flex items-center gap-2">
              <Input id={id} value={r.maxReceives} onChange={(e) => put({ maxReceives: e.target.value })} inputMode="numeric" placeholder="No limit" className="bg-card font-mono" autoComplete="off" data-testid="retry-maxReceives" />
              <span className="w-14 text-[13px] text-muted-foreground">per second</span>
            </div>
          )}
        </Field>
      </div>
      {show && p.retry && <p className="text-[13px] text-bad" role="alert">{p.retry}</p>}
      <RuleHelp><b>Applied to HTTP and HTTPS subscriptions.</b> doze-aws retries a failed POST under this policy; a backoffFunction other than linear is approximated by linear. <b>Stored, not applied:</b> maxReceivesPerSecond (there is no throttling) and headerContentType (the POST is always text/plain; charset=UTF-8). Amazon SQS and AWS Lambda deliveries are one synchronous attempt. On AWS the total retry time for an HTTP/S endpoint cannot exceed 3,600 seconds.</RuleHelp>
    </FormSection>
  );
}

const OFF: Record<string, string> = {
  Firehose: "Not available in doze-aws: it needs Amazon Data Firehose, which doze-aws does not have.",
  Application: "Not available in doze-aws: mobile push needs the platform push services. Messages to an application endpoint are accepted and sent nowhere.",
};
const NAMES: Record<Logged, string> = { HTTP: "HTTP", Lambda: "AWS Lambda", SQS: "Amazon SQS" };

/** The service a delivery logging role must be assumable by. */
export const SNS_PRINCIPAL = "sns.amazonaws.com";

/** The role a protocol's delivery logging is written with: chosen from the roles whose trust policy lets SNS assume them (the server
 * narrows the list), or its ARN typed (a role of another account, or one the console cannot see the trust of). */
function RolePick({ id, value, onChange, label, account }: { id: string; value: string; onChange: (v: string) => void; label: string; account: string }) {
  const roles = useResourceList("iam", "roles", false, { trusts: SNS_PRINCIPAL });
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = roles;
  const pages = roles.data?.pages.length ?? 0;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage && pages < PAGES) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, pages]);
  const names = roles.data?.pages.flatMap((pg) => pg.rows).map((r) => r.name).sort() ?? [];
  return (
    <div className="grid gap-1">
      <Combobox id={id} options={names.map((n) => ({ value: `arn:aws:iam::${account}:role/${n}`, label: n }))} value={value || null} onChange={(v) => onChange(v ?? "")} placeholder={roles.isPending ? "Reading roles…" : "Choose an IAM role"} search="Search roles, or enter an ARN…" empty="No role matches." className="w-full font-mono" pattern aria-label={label} />
      <p className="text-[12.5px] text-muted-foreground" data-testid="role-trust-note">
        {roles.isPending ? "Reading roles…" : names.length === 0 ? <>No role trusts <span className="font-mono">{SNS_PRINCIPAL}</span> yet, so SNS could not assume one. Make a role with that trust policy, or enter an ARN.</> : <>Only roles whose trust policy lets <span className="font-mono">{SNS_PRINCIPAL}</span> assume them are listed. To use another role, enter its ARN.</>}
      </p>
    </div>
  );
}

/** Per protocol: log its deliveries, at what sample rate, with which roles. Stored, not applied: doze-aws writes no delivery logs. */
export function Logging({ f, set, p, show, collapsed, account }: FieldProps & { collapsed: boolean; account: string }) {
  const on = LOGGED.filter((l) => f.logging[l].on).map((l) => NAMES[l]);
  const put = (l: Logged, patch: Partial<TopicLog>) => set({ logging: { ...f.logging, [l]: { ...f.logging[l], ...patch } } });
  return (
    <FormSection title="Delivery status logging" collapsible={collapsed} defaultOpen={!collapsed} description="Write delivery results to CloudWatch Logs." summary={on.length > 0 ? `On for ${on.join(", ")}.` : "Off"} testId="section-logging">
      {(["Firehose", "HTTP", "Lambda", "Application", "SQS"] as const).map((l) => {
        const off = l === "Firehose" || l === "Application";
        const g = off ? null : f.logging[l as Logged];
        const name = off ? (l === "Firehose" ? "Amazon Data Firehose" : "Platform application endpoint") : NAMES[l as Logged];
        return (
          <div key={l} className="grid gap-2" data-testid={`logging-${l}`}>
            <div className="flex items-start gap-3">
              <Checkbox id={`log-${l}`} checked={g?.on ?? false} disabled={off} onCheckedChange={(v) => !off && put(l as Logged, { on: v === true })} className="mt-0.5" data-testid={`log-${l}`} />
              <div className="grid gap-0.5">
                <Label htmlFor={`log-${l}`} className={off ? "text-muted-foreground" : undefined}>{name}{off && <Badge variant="secondary" className="font-normal text-muted-foreground">Disabled</Badge>}</Label>
                {off && <span className="text-[13px] text-muted-foreground">{OFF[l]}</span>}
              </div>
            </div>
            {g?.on && (
              <div className="ml-7 grid gap-4 rounded-xl border bg-well p-4">
                <Field label="Success sample rate" help={<><span className="font-mono">{l}SuccessFeedbackSampleRate</span>, the percentage (0-100) of successfully delivered messages to sample.</>} testId={`field-${l}-rate`}>
                  {(id) => <div className="flex items-center gap-2"><Input id={id} value={g.rate} onChange={(e) => put(l as Logged, { rate: e.target.value })} inputMode="numeric" className="w-28 bg-card font-mono" data-testid={`log-${l}-rate`} /><span className="text-[13px] text-muted-foreground">% of successful deliveries</span></div>}
                </Field>
                <Field label={`${l}SuccessFeedbackRoleArn`} help="Gives Amazon SNS write access to CloudWatch Logs for successful deliveries. Setting it is the switch: the role itself gates nothing." testId={`field-${l}-success`}>
                  {(id) => <RolePick id={id} value={g.success} onChange={(v) => put(l as Logged, { success: v })} label={`${l} success role`} account={account} />}
                </Field>
                <Field label={`${l}FailureFeedbackRoleArn`} help="Once set, every failed delivery generates CloudWatch Logs." testId={`field-${l}-failure`}>
                  {(id) => <RolePick id={id} value={g.failure} onChange={(v) => put(l as Logged, { failure: v })} label={`${l} failure role`} account={account} />}
                </Field>
              </div>
            )}
          </div>
        );
      })}
      {show && p.logging && <p className="text-[13px] text-bad" role="alert">{p.logging}</p>}
      <LocalStatus state="stored" reason="The roles and sample rates are recorded on the topic and returned. doze-aws writes no delivery logs to CloudWatch Logs." />
    </FormSection>
  );
}

type TopicLog = FieldProps["f"]["logging"]["HTTP"];

/** Active tracing: standard topics only. Stored, not applied. */
export function Tracing({ f, set, collapsed }: Pick<FieldProps, "f" | "set"> & { collapsed: boolean }) {
  return (
    <FormSection title="Active tracing" collapsible={collapsed} defaultOpen={!collapsed} summary={f.tracing ? "On" : "Off"} testId="section-tracing">
      <div className="flex items-start gap-3">
        <Switch id="tracing-on" checked={f.tracing} onCheckedChange={(tracing) => set({ tracing })} className="mt-0.5" data-testid="tracing-on" />
        <div className="grid gap-0.5">
          <Label htmlFor="tracing-on">Use active tracing</Label>
          <RuleHelp>Trace requests through the topic with AWS X-Ray. Attribute: <span className="font-mono">TracingConfig</span>, PassThrough by default; standard topics only. Off means PassThrough: the topic passes through the tracing header it receives from a publisher to its subscriptions.</RuleHelp>
        </div>
      </div>
      <LocalStatus state="stored" reason="doze-aws has no X-Ray, so the setting is only recorded." />
    </FormSection>
  );
}

/** The tags: new ones on a create, or the topic's own on an edit (saved with TagResource and UntagResource). */
export function TagsSection({ f, set, show, collapsed }: Pick<FieldProps, "f" | "set" | "show"> & { collapsed: boolean }) {
  const n = filled(f.tags).length;
  return (
    <FormSection title="Tags" collapsible={collapsed} defaultOpen={!collapsed} description="Metadata for cost allocation, access control and search. Do not put sensitive data in tags." summary={n > 0 ? `${n} ${n === 1 ? "tag" : "tags"}` : "No tags"} testId="section-tags">
      <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} addLabel="Add tag" removeLabel="Remove tag" showErrors={show} testId="form-tags" />
      <RuleHelp>Key 1 to 128 characters, value 0 to 256. More than 50 tags on a topic is refused with TagLimitExceeded. Tags change nothing about delivery.</RuleHelp>
    </FormSection>
  );
}
