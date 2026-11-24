// The tabs of a topic's page after Subscriptions: Access policy, Delivery retry policy (HTTP/S), Delivery status logging, Encryption and Archive policy
// (sns__topic-access, topic-delivery-policy, topic-logging, topic-encryption, topic-archive). Each says what is set, what doze-aws does with it,
// and where to change it. Tags are the kit's.
import { PencilIcon } from "lucide-react";
import { CodeBlock, pretty } from "@doze/ui/components/app/code-block";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { useResourceDetail } from "../../../api/resources";
import { LocalStatus } from "../../../kit";
import type { CallRecord } from "../../../api/types/CallRecord";
import { Block } from "./parts";
import { publishers } from "./topic-parts";
import { ResourceTo } from "../value";
import { deliveryRows, hasOwnDelivery, type Logged, archiveOf, daysWords, roleName, type PolicyRow, type Topic } from "./topic-model";

export type TabProps = { t: Topic; edit: () => void };

const EditButton = ({ onClick }: { onClick: () => void }) => <Button type="button" variant="outline" className="bg-card" onClick={onClick} data-testid="tab-edit"><PencilIcon /> Edit</Button>;

const ROWS: DataColumn<PolicyRow>[] = [
  { id: "attribute", header: "Attribute", size: 230, accessorKey: "attribute", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.attribute}</span> },
  { id: "value", header: "Value", size: 240, accessorKey: "value" },
  { id: "constraint", header: "Constraint", accessorKey: "constraint", cell: (c) => <span className="text-muted-foreground">{c.row.original.constraint}</span> },
];

export function AccessTab({ t, edit }: TabProps) {
  return (
    <div className="grid gap-4" data-testid="access-tab">
      <Block title="Access policy" lede="Who can publish to this topic and who can subscribe to it. This is the topic's Policy attribute." actions={<EditButton onClick={edit} />}>
        {t.policy ? <CodeBlock text={pretty(t.policy)} max="max-h-none" testId="policy-json" /> : <p className="text-[13.5px] text-muted-foreground">No policy is set.</p>}
      </Block>
      <p className="max-w-[56rem] text-[13.5px] leading-[21px] text-muted-foreground">By default, only the topic owner can publish or subscribe to the topic. When you create a topic in the console, the default policy uses the aws:SourceOwner condition key. To change it, choose Edit and replace the JSON; the API can also add or remove one statement at a time with AddPermission and RemovePermission.</p>
    </div>
  );
}

/** What AWS does on its own for an endpoint SNS manages: the schedule of the SQS and Lambda deliveries, which doze-aws does not follow. */
const AWS_SCHEDULE: [string, string, string][] = [
  ["Immediate retry", "3 times", "No delay"],
  ["Pre-backoff", "2 times", "1 second apart"],
  ["Backoff", "10 times", "Exponential, from 1 second to 20 seconds"],
  ["Post-backoff", "100,000 times", "20 seconds apart"],
  ["Total", "100,015 attempts", "Over 23 days"],
];

export function RetryTab({ t, edit, http }: TabProps & { http: number }) {
  const own = hasOwnDelivery(t.own);
  return (
    <div className="grid gap-8" data-testid="retry-tab">
      <Block title="Delivery retry policy (HTTP/S)" lede={<>How Amazon SNS retries failed deliveries to HTTP/S endpoints. Read from the topic's DeliveryPolicy attribute; with none set, the defaults below are what an HTTP/S subscription gets (EffectiveDeliveryPolicy).</>} actions={<EditButton onClick={edit} />}>
        <DataTable aria-label="Delivery retry policy" data={deliveryRows(t.effective)} columns={ROWS} rowId={(r) => r.attribute} maxHeight={520} />
        <p className="flex flex-wrap items-center gap-2 text-[13px] leading-5 text-muted-foreground">
          <Badge variant="outline" className="font-normal" data-testid="retry-applied">Applied locally</Badge>
          {own ? "This topic's own policy is in force" : "No policy of its own: the defaults are in force"}. doze-aws retries a failed HTTP or HTTPS POST under these attributes ({http === 0 ? "none of this topic's subscriptions is one" : `${http} of this topic's subscriptions ${http === 1 ? "is" : "are"} one`}); a backoffFunction other than linear is approximated by linear. The total policy retry time for an HTTP/S endpoint cannot be greater than 3,600 seconds. A subscription can carry its own policy; a topic-level one applies to all HTTP/S subscriptions of the topic.
        </p>
        <p className="flex flex-wrap items-center gap-2 text-[13px] leading-5 text-muted-foreground" data-testid="retry-stored">
          <Badge variant="outline" className="font-normal">Stored, not applied</Badge>
          <span><span className="font-mono text-[12.5px]">maxReceivesPerSecond</span> and <span className="font-mono text-[12.5px]">headerContentType</span> are kept and read back, but doze-aws does not throttle deliveries and always posts as text/plain; charset=UTF-8.</span>
        </p>
      </Block>
      <Block title="What SNS does on AWS for the endpoints this topic has" lede="Amazon SQS and AWS Lambda are AWS managed endpoints. Except for HTTP/S, you can't change Amazon SNS-defined delivery policies. doze-aws makes one synchronous attempt for them and does not follow this schedule.">
        <DataTable aria-label="AWS delivery schedule" data={AWS_SCHEDULE.map(([phase, attempts, spacing]) => ({ phase, attempts, spacing }))} columns={[{ id: "phase", header: "Phase", size: 230, accessorKey: "phase" }, { id: "attempts", header: "Attempts", size: 240, accessorKey: "attempts" }, { id: "spacing", header: "Spacing", accessorKey: "spacing" }]} rowId={(r) => r.phase} maxHeight={320} />
      </Block>
    </div>
  );
}

type LogRow = { protocol: string; rate: string; roles: React.ReactNode; off?: string };

export function LoggingTab({ t, edit }: TabProps) {
  const row = (l: Logged): LogRow => {
    const fb = t.feedback[l];
    const name = l === "HTTP" ? "HTTP" : l === "Lambda" ? "AWS Lambda" : "Amazon SQS";
    const on = fb.success !== null || fb.failure !== null;
    return { protocol: name, rate: fb.rate === null ? "Not set" : `${fb.rate}%`, roles: on ? <span className="grid font-mono text-[12.5px]">{fb.success && <span>Success: {roleName(fb.success)}</span>}{fb.failure && <span>Failure: {roleName(fb.failure)}</span>}</span> : "Not set" };
  };
  const rows: LogRow[] = [
    { protocol: "Amazon Data Firehose", rate: "", roles: "", off: "Not available in doze-aws: it needs Amazon Data Firehose, which doze-aws does not have." },
    row("HTTP"),
    row("Lambda"),
    { protocol: "Platform application endpoint", rate: "", roles: "", off: "Not available in doze-aws: mobile push needs the platform push services. Messages to an application endpoint are accepted and sent nowhere." },
    row("SQS"),
  ];
  return (
    <div className="grid gap-4" data-testid="logging-tab">
      <Block title="Delivery status logging" lede="Delivery results written to CloudWatch Logs, per protocol. Read from the topic's SuccessFeedbackRoleArn, SuccessFeedbackSampleRate and FailureFeedbackRoleArn attributes." actions={<EditButton onClick={edit} />}>
        <div className="overflow-x-auto rounded-[var(--radius-card)] border bg-card">
          <table className="w-full min-w-[40rem] border-collapse" aria-label="Delivery status logging">
            <thead className="bg-muted/50"><tr>{["Protocol", "Success sample rate", "IAM roles"].map((h) => <th key={h} className="px-4 py-3 text-left text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{h}</th>)}</tr></thead>
            <tbody className="divide-y">
              {rows.map((r) => (
                <tr key={r.protocol} data-row>
                  <td className="w-[22rem] px-4 py-3.5 align-top text-[13.5px] font-medium text-bright"><span className={r.off ? "text-bright" : undefined}>{r.protocol}</span>{r.off && <Badge variant="secondary" className="ml-2 font-normal text-muted-foreground">Disabled</Badge>}</td>
                  {r.off ? <td colSpan={2} className="px-4 py-3.5 align-top text-[13.5px] text-muted-foreground">{r.off}</td> : <><td className="px-4 py-3.5 align-top text-[13.5px] text-muted-foreground">{r.rate}</td><td className="px-4 py-3.5 align-top text-[13.5px] text-muted-foreground">{r.roles}</td></>}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Block>
      <LocalStatus state="stored" reason="Setting a role is the switch on AWS: a success role writes one record per successful delivery, sampled by the success sample rate (0 to 100), and a failure role writes every failed attempt. doze-aws records the roles and rates and returns them; it writes no delivery logs." />
    </div>
  );
}

export function EncryptionTab({ t, edit, calls = [] }: TabProps & { calls?: readonly CallRecord[] }) {
  const from = publishers(calls);
  const key = useResourceDetail("kms", "keys", t.kms ?? "", t.kms !== null);
  const f = (label: string) => key.data?.fields.find((x) => x.label === label)?.value;
  const alias = t.kms?.startsWith("alias/");
  const rows: [string, React.ReactNode][] = [
    ["Encryption status", t.kms ? <span key="st" className="inline-flex items-center gap-2 text-link"><span aria-hidden className="size-2 rounded-full bg-current" />Enabled</span> : "Disabled"],
    ...(t.kms ? ([
      [alias ? "Customer managed key" : "KMS key", <span key="k" className="font-mono">{key.data ? <ResourceTo to={{ service: "kms", kind: "keys", name: t.kms }}>{t.kms}</ResourceTo> : t.kms}</span>],
      ["Account", <span key="ac" className="font-mono">{t.owner ?? "–"}</span>],
      ...(f("ARN") ? ([["Customer managed key ARN", <span key="a" className="font-mono break-all">{f("ARN")}</span>]] as [string, React.ReactNode][]) : []),
      ...(f("Description") ? ([["Description", f("Description")]] as [string, React.ReactNode][]) : []),
    ] as [string, React.ReactNode][]) : []),
  ];
  return (
    <div className="grid gap-4" data-testid="encryption-tab">
      <Block title="Encryption" lede="Server-side encryption with AWS KMS. Read from the topic's KmsMasterKeyId attribute." actions={<EditButton onClick={edit} />}>
        {t.kms && (
          <nav aria-label="Wired to" className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[var(--radius-card)] border bg-card px-5 py-3" data-testid="kms-strip">
            {from.length > 0 && <><span className="text-[13.5px] text-muted-foreground">Publisher</span>{from.map((l) => <span key={l} className="inline-flex rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright">{l}</span>)}<span aria-hidden className="text-input">→</span></>}
            <span className="inline-flex rounded-lg border border-primary/30 bg-pick px-3 py-1.5 font-mono text-[13px] text-bright">{t.name}</span>
            <span aria-hidden className="text-input">→</span>
            <span className="text-[13.5px] text-muted-foreground">KMS key</span>
            <span className="inline-flex rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright">{t.kms}</span>
          </nav>
        )}
        <dl className="grid grid-cols-[minmax(11rem,15rem)_minmax(0,1fr)] rounded-[var(--radius-card)] border bg-card px-6 py-2 text-[13.5px] [&_dd]:border-b [&_dd]:py-3 [&_dt]:border-b [&_dt]:py-3 [&>span:last-child>*]:border-b-0">
          {rows.map(([l, v]) => (<span key={l} className="contents"><dt className="text-muted-foreground">{l}</dt><dd className="text-bright" data-testid={`field-${l}`}>{v}</dd></span>))}
        </dl>
      </Block>
      <LocalStatus state="stored" reason={<>The key is recorded on the topic; publishes are not encrypted or checked against the key here. On AWS a custom key needs a key policy that lets sns.amazonaws.com call kms:Decrypt and kms:GenerateDataKey.</>} />
    </div>
  );
}

export function ArchiveTab({ t, edit }: TabProps) {
  const a = archiveOf(t.archive);
  return (
    <div className="grid gap-4" data-testid="archive-tab">
      <Block title="Archive policy" lede="Message archiving keeps a single copy of all messages published to the topic, for a minimum of one day to a maximum of 365 days. Read from the ArchivePolicy attribute." actions={<EditButton onClick={edit} />}>
        <dl className="grid grid-cols-[minmax(11rem,15rem)_minmax(0,1fr)] rounded-[var(--radius-card)] border bg-card px-6 py-2 text-[13.5px] [&>*]:border-b [&>*]:py-3 [&>:nth-last-child(-n+2)]:border-b-0">
          <dt className="text-muted-foreground">Status</dt><dd className={a.active ? "text-link" : "text-muted-foreground"} data-testid="field-Status"><span className="inline-flex items-center gap-2"><span aria-hidden className="size-2 rounded-full bg-current" />{a.active ? "active" : "inactive"}</span></dd>
          {a.days !== null && (<><dt className="text-muted-foreground">Message retention period</dt><dd className="text-bright" data-testid="field-retention">{daysWords(a.days)}</dd></>)}
        </dl>
        {t.archive && (<div className="grid gap-1.5"><span className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">JSON preview</span><CodeBlock text={oneLine(t.archive)} max="max-h-40" testId="archive-json" /></div>)}
      </Block>
      <LocalStatus state="stored" reason="The status shows inactive when the archive policy is an empty JSON object. Nothing is archived and nothing can be replayed locally." />
    </div>
  );
}

/** A small JSON object on one line, as the board shows it: `{ "MessageRetentionPeriod": "7" }`. */
function oneLine(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text)).replace(/^\{(.)/, "{ $1").replace(/(.)\}$/, "$1 }").replace(/":/g, '": ');
  } catch {
    return text;
  }
}
