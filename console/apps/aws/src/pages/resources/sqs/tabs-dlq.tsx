// The Dead-letter queue and Encryption tabs of a queue's page (sqs__queue-detail, dlq-detail, queue-encryption): two-column cards with
// the teaching note beside the fields.
import { InfoIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { useResourceDetail } from "../../../api/resources";
import { LocalStatus } from "../../../kit";
import { RecentCalls } from "../../../kit/recent-calls";
import { ResourceTo } from "../value";
import { Block } from "./parts";
import { queueOf, wait } from "./queue-model";
import { allowWords } from "./redrive-model";
import { RedriveStatus } from "./redrive-status";
import type { TabProps } from "./tabs";

const dl = "grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] [&>*]:border-b [&>*]:py-3 [&>:nth-last-child(-n+2)]:border-b-0 text-[13.5px]";
const card = "rounded-[14px] border bg-card p-6 lg:p-7";
const heading = "text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright";

/** The dead-letter queue this queue sends to, who may send to it, and (when it is one) its redrive. */
export function DlqTab({ d, q, go, dead }: TabProps) {
  const target = useResourceDetail("sqs", "queues", q.dlq ?? "", q.dlq !== null);
  const to = q.dlq && target.data ? queueOf(target.data) : null;
  const holds = to ? to.available + to.inFlight + to.delayed : 0;
  return (
    <div className="grid gap-8" data-testid="dlq-tab">
      {dead.length > 0 && <RedriveStatus d={d} q={q} sources={dead} go={go} />}
      {q.dlq ? (
        <section className={card} aria-label="Dead-letter queue">
          <h2 className={heading}>Dead-letter queue</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">Where messages go after too many failed receives. Set in the RedrivePolicy attribute.</p>
          <div className="mt-4 grid gap-x-10 gap-y-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <dl className={dl}>
              <dt className="text-muted-foreground">Dead-letter queue</dt>
              <dd><ResourceTo to={{ service: "sqs", kind: "queues", name: q.dlq }} className="rounded-full border bg-well px-2.5 py-0.5 font-mono text-[12.5px]">{q.dlq}</ResourceTo></dd>
              <dt className="text-muted-foreground">ARN</dt>
              <dd className="font-mono break-all text-bright">{target.data ? queueOf(target.data).arn : "–"}</dd>
              <dt className="text-muted-foreground">Maximum receives</dt>
              <dd className="text-bright">{q.maxReceives}</dd>
            </dl>
            <p className="border-l-2 pl-4 text-[13.5px] leading-[21px] text-muted-foreground" data-testid="dlq-words">
              <Emphasis
                text={`A message received more than ${q.maxReceives} times without being deleted moves to ${q.dlq}${to ? `, which holds ${holds} ${holds === 1 ? "message" : "messages"} now and keeps messages for ${wait(to.retention)}` : ""}. Viewing a message in the console counts as a receive, so browsing can push messages here.`}
                names={[q.dlq]}
                bold={[/(?<=more than )\d+/, /\d[\d,]* messages?(?= now)/, /\d+ (?:days?|hours?|minutes?|weeks?)(?=\.)/]}
              />
            </p>
          </div>
        </section>
      ) : dead.length > 0 ? null : (
        <Alert icon={InfoIcon} tone="info" title="No dead-letter queue." testId="dlq-none">
          A message that keeps failing stays in {q.name} until its {wait(q.retention)} retention ends. Edit the queue to add a{q.fifo ? " FIFO" : ""} dead-letter queue.
          <div className="mt-3"><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => go("edit")}>Edit queue</Button></div>
        </Alert>
      )}
      <Block title="Redrive allow policy" lede={`Which source queues may use ${q.name} as THEIR dead-letter queue.`} testId="allow-reading">
        <dl className={`${dl} rounded-[14px] border bg-card px-6 py-0.5`}>
          <dt className="text-muted-foreground">Redrive allow policy</dt>
          <dd className="text-bright">{q.allow === null ? `Not set (all source queues may use ${q.name} as a dead-letter queue)` : allowWords(q.allow)}</dd>
        </dl>
      </Block>
      <RecentCalls calls={d.calls} noun="queue" />
    </div>
  );
}

/** The key manager as the board words it: `Customer managed`, `AWS managed`. */
const managed = (v: string | null | undefined): string | null | undefined => (v === "Customer" ? "Customer managed" : v === "AWS" ? "AWS managed" : v);

export function EncryptionTab({ q }: TabProps) {
  const key = useResourceDetail("kms", "keys", q.kms ?? "", q.kms !== null);
  const f = (l: string) => key.data?.fields.find((x) => x.label === l)?.value;
  const kind = key.data ? [managed(f("Managed by")), f("Key spec") === "SYMMETRIC_DEFAULT" ? "symmetric" : f("Key spec")?.toLowerCase(), f("State")?.toLowerCase()].filter(Boolean).join(", ") : null;
  const on = q.sse !== "none";
  return (
    <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_22rem]" data-testid="encryption-tab">
      <section className={card} aria-label="Encryption">
        <h2 className={heading}>Encryption</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">The encryption settings stored on the queue.</p>
        <dl className="mt-4 grid grid-cols-[minmax(11rem,14rem)_minmax(0,1fr)] text-[13.5px] [&>*]:border-b [&>*]:py-4 [&>:nth-last-child(-n+2)]:border-b-0">
          <dt className="text-muted-foreground">Server-side encryption</dt>
          <dd data-testid="sse-state">
            <StatusDot tone={on ? "ok" : "idle"} className="text-[13.5px]">{on ? "Enabled" : "Disabled"}</StatusDot>
            {on && <div className="mt-0.5 text-muted-foreground">{q.sse === "kms" ? "AWS Key Management Service key (SSE-KMS)" : "Amazon SQS key (SSE-SQS)"}</div>}
          </dd>
          {q.kms && (
            <>
              <dt className="text-muted-foreground">KMS key</dt>
              <dd className="text-bright">
                {key.data ? <ResourceTo to={{ service: "kms", kind: "keys", name: q.kms }} className="font-mono">{q.kms}</ResourceTo> : <span className="font-mono">{q.kms}</span>}
                {kind && <div className="mt-0.5 text-muted-foreground">{kind[0]!.toUpperCase()}{kind.slice(1)}</div>}
              </dd>
            </>
          )}
          {q.sse === "kms" && q.reuse !== null && (<><dt className="text-muted-foreground">Data key reuse period</dt><dd className="text-bright">{wait(q.reuse)}</dd></>)}
        </dl>
      </section>
      <aside className={card} aria-label="Stored, not applied">
        <h2 className={heading}>Stored, not applied</h2>
        <LocalStatus state="stored" className="mt-3 border-l-2 pl-4 text-[13.5px] leading-[21px]" reason="doze-aws keeps SqsManagedSseEnabled, KmsMasterKeyId and the data key reuse period and returns them, so a template or an app reads what it wrote. No message is encrypted, so no key policy is consulted." />
      </aside>
    </div>
  );
}
