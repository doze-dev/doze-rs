// The tabs of a queue's page: SNS subscriptions, Lambda triggers, Access policy (the Dead-letter queue and Encryption tabs are in
// ./tabs-dlq.tsx; the Tagging tab is the kit's). Each says what is there, or, when nothing is, what that means for the queue and how
// to change it, under the same "(0)" header and top-right button the populated tab has.
import { PlusIcon, ShieldIcon, WorkflowIcon, ZapIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock, pretty } from "@doze/ui/components/app/code-block";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { ResourceTo, Value } from "../value";
import { Block } from "./parts";
import { whoLetsIn } from "./policy-reading";
import type { Queue } from "./queue-model";
import { queueEndpoints, subscribedWords } from "./sns-words";

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
  const subscribe = <Button type="button" className="bg-card" variant="outline" onClick={() => go("sns-subscribe")} data-testid="sns-subscribe-open"><PlusIcon /> Subscribe to Amazon SNS topic</Button>;
  if (s?.error) return <Alert tone="warn" title="The subscriptions could not be read">{s.error}</Alert>;
  if (!s || s.rows.length === 0) {
    return <SnsEmpty q={q} topics={topics.data?.pages.flatMap((p) => p.rows) ?? []} subscribe={subscribe} />;
  }
  return (
    <Block title={`Subscriptions (${s.rows.length})`} actions={subscribe} testId="sns-subscriptions">
      <DataTable aria-label="Subscriptions" data={s.rows} columns={subColumns(d, "SNS subscriptions")} rowId={(r) => r.cells[1] ?? r.name} maxHeight={420} />
      {s.more && <p className="text-[13px] text-muted-foreground">The first page only; there are more.</p>}
    </Block>
  );
}

/** The empty tab: what it means, and (when a topic has subscribers) where that topic delivers. */
function SnsEmpty({ q, topics, subscribe }: { q: Queue; topics: readonly ResourceRow[]; subscribe: React.ReactNode }) {
  const top = topics.filter((r) => Number(r.cells[1]) > 0).sort((a, b) => Number(b.cells[1]) - Number(a.cells[1]))[0];
  const detail = useResourceDetail("sns", "topics", top?.name ?? "", Boolean(top));
  const queues = queueEndpoints(detail.data?.subs.find((x) => x.title === "Subscriptions")?.rows.map((r) => r.cells[2]) ?? []);
  const who = top ? subscribedWords(top.name, Number(top.cells[1]), queues) : "";
  return (
    <Block title="Subscriptions (0)" actions={subscribe} testId="sns-subscriptions">
      <EmptyTab icon={WorkflowIcon} title="No SNS subscriptions" action={subscribe} testId="sns-empty">
        {q.name} is not subscribed to any Amazon SNS topic, so nothing published to a topic reaches it.
        {who ? <> <Emphasis text={who} names={top ? [top.name, ...queues] : []} /></> : ""}
      </EmptyTab>
    </Block>
  );
}

export function LambdaTab({ d, q, go }: TabProps) {
  const s = sub(d, "Lambda triggers");
  const configure = <Button type="button" variant="outline" className="bg-card" onClick={() => go("lambda-configure")} data-testid="lambda-configure-open"><PlusIcon /> Configure Lambda function trigger</Button>;
  if (s?.error) return <Alert tone="warn" title="The Lambda triggers could not be read">{s.error}</Alert>;
  if (!s || s.rows.length === 0) {
    return (
      <Block title="Lambda triggers (0)" actions={configure} testId="lambda-triggers">
        <EmptyTab icon={ZapIcon} title="No Lambda triggers" action={configure} testId="lambda-empty">
          No Lambda function polls {q.name}. Once one does, this tab lists the function and its status.
        </EmptyTab>
      </Block>
    );
  }
  return (
    <Block title={`Lambda triggers (${s.rows.length})`} actions={configure} testId="lambda-triggers">
      <DataTable aria-label="Lambda triggers" data={s.rows} columns={subColumns(d, "Lambda triggers")} rowId={(r) => r.cells[3] ?? r.name} maxHeight={420} />
    </Block>
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
