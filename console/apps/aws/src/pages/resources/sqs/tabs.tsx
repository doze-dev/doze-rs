// The tabs of a queue's page: SNS subscriptions, Lambda triggers, Dead-letter queue, Access policy, Encryption (the Tagging tab is
// the kit's). Each says what is there, or, when nothing is, what that means for the queue and how to change it.
import { InfoIcon, PlusIcon, ShieldIcon, WorkflowIcon, ZapIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock, pretty } from "@doze/ui/components/app/code-block";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { LocalStatus } from "../../../kit";
import { ResourceTo, Value } from "../value";
import { Block, RecentCalls } from "./parts";
import { whoLetsIn } from "./policy-reading";
import { queueOf, type Queue, wait } from "./queue-model";
import { RedriveStatus } from "./redrive-status";

export type TabProps = { d: ResourceDetail; q: Queue; go: (view: string) => void; account: string; dead: string[] };

const sub = (d: ResourceDetail, title: string) => d.subs.find((s) => s.title === title);

const subColumns = (d: ResourceDetail, title: string): DataColumn<ResourceRow>[] =>
  (sub(d, title)?.columns ?? []).map((c, i) => ({
    id: `c${i}`,
    header: c.title,
    accessorFn: (r) => r.cells[i] ?? "",
    meta: { mono: c.format === "mono", numeric: c.format === "number" },
    cell: (cell) => {
      const r = cell.row.original;
      const v = <Value value={r.cells[i] ?? null} format={c.format} />;
      return i === 0 && r.link ? <ResourceTo to={r.link}>{v}</ResourceTo> : v;
    },
  }));

export function SnsTab({ d, q, go }: TabProps) {
  const s = sub(d, "SNS subscriptions");
  const topics = useResourceList("sns", "topics");
  const withSubs = (topics.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => Number(r.cells[1]) > 0).sort((a, b) => Number(b.cells[1]) - Number(a.cells[1]));
  const subscribe = <Button type="button" className="bg-card" variant="outline" onClick={() => go("sns-subscribe")} data-testid="sns-subscribe-open"><PlusIcon /> Subscribe to Amazon SNS topic</Button>;
  if (s?.error) return <Alert tone="warn" title="The subscriptions could not be read">{s.error}</Alert>;
  if (!s || s.rows.length === 0) {
    const top = withSubs[0];
    return (
      <EmptyTab icon={WorkflowIcon} title="No SNS subscriptions" action={subscribe} testId="sns-empty">
        {q.name} is not subscribed to any Amazon SNS topic, so nothing published to a topic reaches it.
        {top ? ` The topic ${top.name} delivers to ${Number(top.cells[1])} ${Number(top.cells[1]) === 1 ? "subscription" : "subscriptions"} today.` : ""}
      </EmptyTab>
    );
  }
  return (
    <Block title={`Subscriptions (${s.rows.length})`} actions={subscribe} testId="sns-subscriptions">
      <DataTable aria-label="Subscriptions" data={s.rows} columns={subColumns(d, "SNS subscriptions")} rowId={(r) => r.cells[1] ?? r.name} maxHeight={420} />
      {s.more && <p className="text-[13px] text-muted-foreground">The first page only; there are more.</p>}
    </Block>
  );
}

export function LambdaTab({ d, q, go }: TabProps) {
  const s = sub(d, "Lambda triggers");
  const configure = <Button type="button" variant="outline" className="bg-card" onClick={() => go("lambda-configure")} data-testid="lambda-configure-open"><PlusIcon /> Configure Lambda function trigger</Button>;
  if (s?.error) return <Alert tone="warn" title="The Lambda triggers could not be read">{s.error}</Alert>;
  if (!s || s.rows.length === 0) {
    return (
      <EmptyTab icon={ZapIcon} title="No Lambda triggers" action={configure} testId="lambda-empty">
        No Lambda function polls {q.name}. Once one does, this tab lists the function and its status.
      </EmptyTab>
    );
  }
  return (
    <Block title={`Lambda triggers (${s.rows.length})`} actions={configure} testId="lambda-triggers">
      <DataTable aria-label="Lambda triggers" data={s.rows} columns={subColumns(d, "Lambda triggers")} rowId={(r) => r.cells[3] ?? r.name} maxHeight={420} />
    </Block>
  );
}

/** The dead-letter queue this queue sends to, who may send to it, and (when it is one) its redrive. */
export function DlqTab({ d, q, go, dead }: TabProps) {
  const target = useResourceDetail("sqs", "queues", q.dlq ?? "", q.dlq !== null);
  const to = q.dlq && target.data ? queueOf(target.data) : null;
  const allowed = q.allow ? (JSON.parse(q.allow) as { redrivePermission?: string; sourceQueueArns?: string[] }) : null;
  return (
    <div className="grid gap-10" data-testid="dlq-tab">
      {dead.length > 0 && <RedriveStatus d={d} q={q} sources={dead} go={go} />}
      {q.dlq ? (
        <Block title="Dead-letter queue" lede="Where messages go after too many failed receives. Set in the RedrivePolicy attribute.">
          <dl className="grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] rounded-[14px] border bg-card px-6 [&>*]:border-b [&>*]:py-3 [&>:nth-last-child(-n+2)]:border-b-0 text-[13.5px]">
            <dt className="text-muted-foreground">Dead-letter queue</dt>
            <dd><ResourceTo to={{ service: "sqs", kind: "queues", name: q.dlq }} className="font-mono">{q.dlq}</ResourceTo></dd>
            <dt className="text-muted-foreground">ARN</dt>
            <dd className="font-mono break-all text-bright">{target.data ? queueOf(target.data).arn : "–"}</dd>
            <dt className="text-muted-foreground">Maximum receives</dt>
            <dd className="text-bright">{q.maxReceives}</dd>
          </dl>
          <p className="text-[13.5px] leading-[21px] text-muted-foreground" data-testid="dlq-words">
            A message received more than {q.maxReceives} times without being deleted moves to <span className="font-mono text-foreground">{q.dlq}</span>{to ? <>, which holds {to.available + to.inFlight + to.delayed} {to.available + to.inFlight + to.delayed === 1 ? "message" : "messages"} now and keeps messages for {wait(to.retention)}</> : null}. Viewing a message in the console counts as a receive, so browsing can push messages here.
          </p>
        </Block>
      ) : dead.length > 0 ? null : (
        <Alert icon={InfoIcon} tone="info" title="No dead-letter queue." testId="dlq-none">
          A message that keeps failing stays in {q.name} until its {wait(q.retention)} retention ends. Edit the queue to add a{q.fifo ? " FIFO" : ""} dead-letter queue.
          <div className="mt-3"><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => go("edit")}>Edit queue</Button></div>
        </Alert>
      )}
      <Block title="Redrive allow policy" lede={`Which source queues may use ${q.name} as THEIR dead-letter queue.`} testId="allow-reading">
        <dl className="grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] rounded-[14px] border bg-card px-6 py-3 text-[13.5px]">
          <dt className="text-muted-foreground">Redrive allow policy</dt>
          <dd className="text-bright">{allowed === null ? `Not set (all source queues may use ${q.name} as a dead-letter queue)` : allowed.redrivePermission === "denyAll" ? "Deny all: no source queue may use it" : allowed.redrivePermission === "byQueue" ? `By queue: ${(allowed.sourceQueueArns ?? []).map((a) => a.split(":").pop()).join(", ")}` : "Allow all source queues"}</dd>
        </dl>
      </Block>
      <RecentCalls d={d} />
    </div>
  );
}

export function PolicyTab({ q, go, account }: TabProps) {
  const reading = whoLetsIn(q.policy, account);
  return (
    <div className="grid gap-8" data-testid="policy-tab">
      <Block title="Access policy" lede="The queue's Policy attribute, as SQS returns it. By default only the queue owner can send and receive, and no statement is needed for that." actions={<Button type="button" variant="outline" className="bg-card" onClick={() => go("edit")} data-testid="policy-edit">Edit</Button>}>
        {q.policy ? <CodeBlock text={pretty(q.policy)} max="max-h-[420px]" testId="policy-json" /> : <EmptyTab icon={ShieldIcon} title="No access policy" testId="policy-none">Only the queue owner can send and receive messages, which needs no statement.</EmptyTab>}
      </Block>
      <Block title="Who this lets in" lede={q.policy ? "Read from the statements above." : "Read from the policy, which is empty."} testId="who-reading">
        <ul className="grid divide-y rounded-[14px] border bg-card text-[13.5px]">
          {reading.map((r, i) => (
            <li key={i} className="grid grid-cols-[minmax(9rem,14rem)_minmax(0,1fr)] gap-4 px-6 py-3" data-testid="who-row">
              <span className="font-medium text-bright">{r.who}</span>
              <span className="text-foreground">{r.what}</span>
            </li>
          ))}
        </ul>
      </Block>
    </div>
  );
}

export function EncryptionTab({ q }: TabProps) {
  const key = useResourceDetail("kms", "keys", q.kms ?? "", q.kms !== null);
  const f = (l: string) => key.data?.fields.find((x) => x.label === l)?.value;
  const kind = key.data ? [f("Managed by"), f("Key spec") === "SYMMETRIC_DEFAULT" ? "symmetric" : f("Key spec")?.toLowerCase(), f("State")?.toLowerCase()].filter(Boolean).join(", ") : null;
  return (
    <div className="grid gap-6" data-testid="encryption-tab">
      <Block title="Encryption" lede="The encryption settings stored on the queue.">
        <dl className="grid grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] rounded-[14px] border bg-card px-6 py-2 text-[13.5px] [&>*]:border-b [&>*]:py-3 [&>:nth-last-child(-n+2)]:border-b-0">
          <dt className="text-muted-foreground">Server-side encryption</dt>
          <dd className="text-bright">{q.sse === "none" ? "Disabled" : q.sse === "kms" ? "Enabled · AWS Key Management Service key (SSE-KMS)" : "Enabled · Amazon SQS key (SSE-SQS)"}</dd>
          {q.kms && (<><dt className="text-muted-foreground">KMS key</dt><dd className="text-bright">{key.data ? <ResourceTo to={{ service: "kms", kind: "keys", name: q.kms }} className="font-mono">{q.kms}</ResourceTo> : <span className="font-mono">{q.kms}</span>}{kind && <span className="ml-2 text-muted-foreground">{kind[0]!.toUpperCase()}{kind.slice(1)}</span>}</dd></>)}
          {q.sse === "kms" && q.reuse !== null && (<><dt className="text-muted-foreground">Data key reuse period</dt><dd className="text-bright">{wait(q.reuse)}</dd></>)}
        </dl>
      </Block>
      <LocalStatus state="stored" reason="doze-aws keeps SqsManagedSseEnabled, KmsMasterKeyId and the data key reuse period and returns them, so a template or an app reads what it wrote. No message is encrypted, so no key policy is consulted." />
    </div>
  );
}
