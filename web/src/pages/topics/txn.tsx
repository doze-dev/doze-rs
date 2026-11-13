// A transaction (boards txn-open, txn-holding; classic pages/producers/txn.rs and hold.rs): what it is, how long it has left,
// what it holds back and what happens if it is not ended, every transaction the broker knows; and, on `/held`, who is held.
import { Link, useParams } from "@tanstack/react-router";
import { InfoIcon, SearchIcon } from "lucide-react";
import { useState } from "react";
import { ApiFailure } from "@/api/client";
import { useTxn } from "@/api/topics";
import type { TxnResponse } from "@/api/types/TxnResponse";
import type { TxnRow } from "@/api/types/TxnRow";
import { EmptyState } from "@/components/app/empty-state";
import { Spans } from "@/components/app/spans";
import { StatusDot, type Tone } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { clock, spanMs, timeoutText } from "@/lib/kafka";
import { PageError } from "../page-error";
import { TailRuler } from "./producers";

const STATES = ["Ongoing", "PrepareCommit", "PrepareAbort", "CompleteCommit", "CompleteAbort", "Empty"];
const tone = (s: string): Tone => (s === "Ongoing" ? "warn" : s === "CompleteCommit" ? "ok" : s === "PrepareAbort" || s === "CompleteAbort" ? "idle" : "info");

function useTxnPage() {
  const { topic, txn } = useParams({ strict: false }) as { topic: string; txn: string };
  return { topic, id: txn, q: useTxn(topic, txn) };
}

function Missing({ topic, id, error, retry }: { topic: string; id: string; error: Error; retry: () => void }) {
  if (error instanceof ApiFailure && error.status === 404) {
    return (
      <EmptyState
        icon={SearchIcon}
        title={`No transaction called ${id}`}
        action={
          <Button asChild>
            <Link to="/topics/$topic/producers" params={{ topic }}>
              Back to the producers
            </Link>
          </Button>
        }
      >
        {error.message}
      </EmptyState>
    );
  }
  return <PageError error={error} onRetry={retry} />;
}

/** What is about this transaction, under the topic's frame: its name, the state (when it is over) and the sentence. */
function Lead({ r, sentence, actions }: { r: TxnResponse; sentence: React.ReactNode; actions?: React.ReactNode }) {
  const x = r.view.txn;
  return (
    <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0 max-w-[56rem]">
        <h2 className="flex flex-wrap items-center gap-3 font-mono text-[19px] font-medium text-bright" data-testid="txn-title">
          {x.id}
          {x.state !== "Ongoing" && <StatusDot tone={tone(x.state)} className="font-sans">{x.state}</StatusDot>}
        </h2>
        <p className="mt-1.5 text-[14px] leading-[22px] text-muted-foreground">{sentence}</p>
      </div>
      {actions}
    </div>
  );
}

function AllTxns({ r }: { r: TxnResponse }) {
  const v = r.view;
  const [only, setOnly] = useState("all");
  const total = v.all.length + v.more;
  const rows = only === "all" ? v.all : v.all.filter((t) => t.state === only);
  const parts = (t: TxnRow) => t.partitions.map(([n, p]) => `${n}-${p}`).join(", ");
  return (
    <section className="mt-10" data-testid="all-txns">
      <h2 className="text-[17px] font-semibold text-bright">Transactions on this broker</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">
        The broker knows {number(total)} {total === 1 ? "transactional id" : "transactional ids"}, by state. ListTransactions answers by state, producer id and how long a transaction has run.
      </p>
      <ToggleGroup type="single" variant="outline" size="sm" value={only} onValueChange={(x) => x && setOnly(x)} aria-label="State" className="mt-4 flex-wrap bg-card" data-testid="txn-states">
        <ToggleGroupItem value="all" className="px-3">
          All {total}
        </ToggleGroupItem>
        {STATES.map((s) => (
          <ToggleGroupItem key={s} value={s} className="px-3">
            {s} {v.all.filter((t) => t.state === s).length}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <div className="mt-3 overflow-x-auto rounded-xl border bg-card">
        <table className="w-full min-w-[48rem] table-fixed text-[13px]">
          <thead className="bg-well text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
            <tr>
              <th className="h-10 px-4 font-medium">Transactional id</th>
              <th className="px-4 font-medium">State</th>
              <th className="px-4 font-medium">Producer</th>
              <th className="px-4 font-medium">Running</th>
              <th className="px-4 font-medium">Timeout</th>
              <th className="px-4 font-medium">Partitions</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((t) => (
              <tr key={t.id} className="h-11 border-t">
                <td className="truncate px-4 font-mono">
                  <Link to="/topics/$topic/transactions/$txn" params={{ topic: v.topic, txn: t.id }} className="text-primary hover:underline">
                    {t.id}
                  </Link>
                </td>
                <td className="px-4">
                  <StatusDot tone={tone(t.state)}>{t.state}</StatusDot>
                </td>
                <td className="px-4 font-mono">
                  {t.producer_id}, epoch {t.producer_epoch}
                </td>
                <td className="px-4 font-mono">{spanMs(Math.max(0, v.now_ms - t.since_ms))}</td>
                <td className="px-4 font-mono">{timeoutText(t.timeout_ms)}</td>
                <td className="truncate px-4 font-mono" title={parts(t)}>
                  {parts(t)}
                </td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={6} className="h-16 px-4 text-center text-muted-foreground">
                  No transaction is in this state.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-[12.5px] text-muted-foreground">
        A transactional id is forgotten after 7 days idle (<code className="font-mono">transactional.id.expiration.ms</code>), once its transaction is over; one with a transaction open never is, because the timeout deals with it first. If it is forgotten, the next InitProducerId makes a new producer id.
        {v.more > 0 && ` ${v.more} more are not listed.`}
      </p>
    </section>
  );
}

function Kv({ rows }: { rows: [string, React.ReactNode, string?][] }) {
  return (
    <dl>
      {rows.map(([dt, dd, sub]) => (
        <div key={dt} className="grid grid-cols-[9rem_minmax(0,1fr)] gap-3 border-b py-2.5 text-[13px] last:border-b-0">
          <dt className="text-muted-foreground">{dt}</dt>
          <dd className="min-w-0">
            {dd}
            {sub && <span className="block text-[12.5px] text-muted-foreground">{sub}</span>}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function TxnPage() {
  const { topic, id, q } = useTxnPage();
  if (q.error && !q.data) return <Missing topic={topic} id={id} error={q.error} retry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-96 w-full rounded-xl" />;
  const r = q.data;
  const v = r.view;
  const x = v.txn;
  const tailPart = v.parts.find((p) => p.partition === v.tail_partition);
  return (
    <div data-testid="txn">
      <Lead r={r} sentence={<Spans spans={r.sentence} />} />
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
        <section className="rounded-xl border bg-card p-5">
          <h3 className="text-[16px] font-semibold text-bright">The transaction</h3>
          <div className="mt-3">
            <Kv
              rows={[
                ["Transactional id", <span className="font-mono">{x.id}</span>],
                ["State", <StatusDot tone={tone(x.state)}>{x.state}</StatusDot>],
                ["Producer", <span className="font-mono">{x.producer_id}, epoch {x.producer_epoch}</span>],
                ["Began", <span className="font-mono">{clock(x.since_ms)} UTC</span>, `${spanMs(r.running_ms)} before this page was fetched; adding partitions does not start the clock again`],
                [
                  "Timeout",
                  <span className="flex max-w-[22rem] items-center gap-3">
                    <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-track">
                      <span className="block h-full rounded-full bg-warn" style={{ width: `${r.timeout_percent}%` }} />
                    </span>
                    <span className="font-mono text-[12.5px] whitespace-nowrap">
                      {spanMs(r.running_ms)} of {r.timeout}
                    </span>
                  </span>,
                  `${r.timeout}: ${r.left_ms > 0 ? `${spanMs(r.left_ms)} left, then the broker aborts it and fences the producer` : "passed: the broker aborts it at its next look and fences the producer"}`,
                ],
                [
                  "Partitions",
                  v.parts.length === 0 && v.elsewhere.length === 0 ? (
                    <span className="text-muted-foreground">none yet</span>
                  ) : (
                    <span className="font-mono">
                      {v.parts.map((p) => (
                        <span key={p.partition} className="block">
                          {v.topic}-{p.partition}
                          {p.first !== null && ` from ${number(p.first)}`}
                        </span>
                      ))}
                      {v.elsewhere.map(([t, p]) => (
                        <span key={`${t}-${p}`} className="block text-muted-foreground">
                          {t}-{p}
                        </span>
                      ))}
                    </span>
                  ),
                ],
                ["Offsets staged", "not listed", "DescribeTransactions does not say whether a group was added; on commit staged offsets are applied to the group, on abort forgotten"],
              ]}
            />
          </div>
        </section>
        <div className="flex min-w-0 flex-col gap-4">
          <section className="rounded-xl border bg-card p-5">
            <h3 className="text-[16px] font-semibold text-bright">What it holds back</h3>
            <p className="mt-2 text-[13.5px]">
              <Spans spans={r.holds} />
            </p>
            <Button variant="outline" size="sm" className="mt-4" asChild>
              <Link to="/topics/$topic/transactions/$txn/held" params={{ topic, txn: id }} data-testid="who-is-held">
                Who is held
              </Link>
            </Button>
          </section>
          <section className="rounded-xl border bg-card p-5">
            <h3 className="text-[16px] font-semibold text-bright">If it is not ended</h3>
            <p className="mt-2 text-[13.5px]">
              <Spans spans={r.unended_before} />
              {r.unended_after.length > 0 && (
                <>
                  <b className="font-semibold text-refused">Refused</b> <code className="font-mono">INVALID_PRODUCER_EPOCH (47)</code>
                  <Spans spans={r.unended_after} />
                </>
              )}
            </p>
          </section>
        </div>
      </div>
      {(r.held > 0 || v.tail.cells.length > 0) && tailPart && (
        <section className="mt-10">
          <h2 className="text-[17px] font-semibold text-bright">
            {v.topic}-{v.tail_partition}, the tail
          </h2>
          <p className="mt-1 mb-4 text-[13.5px] text-muted-foreground">
            Offsets {number(v.tail.from)} to {number(v.tail.from + v.tail.cells.length - 1)}.
          </p>
          <TailRuler t={v.tail} lastStable={tailPart.last_stable} end={tailPart.end} />
        </section>
      )}
      <div className="mt-6 flex gap-3 rounded-xl border bg-card p-4 text-[13.5px]">
        <InfoIcon className="mt-0.5 size-4.5 shrink-0 text-cool" aria-hidden />
        <p>Only the producer, by ending the transaction, or the timeout can end it. The console only reads it: ending it is the producer's job, or the timeout's.</p>
      </div>
      <AllTxns r={r} />
    </div>
  );
}

export function TxnHeldPage() {
  const { topic, id, q } = useTxnPage();
  if (q.error && !q.data) return <Missing topic={topic} id={id} error={q.error} retry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-96 w-full rounded-xl" />;
  const r = q.data;
  const v = r.view;
  const p = v.parts.find((x) => x.partition === v.tail_partition);
  const end = p?.end ?? 0;
  const stable = p?.last_stable ?? 0;
  const here = p && p.first !== null ? Math.max(0, p.end - Math.max(p.first, p.start)) : 0;
  return (
    <div data-testid="txn-held">
      <Lead
        r={r}
        sentence={<Spans spans={r.holding} />}
        actions={
          <Button variant="outline" asChild>
            <Link to="/topics/$topic/messages" params={{ topic }} search={{ isolation: "read_committed", partition: String(v.tail_partition) }}>
              The same, as a consumer sees it
            </Link>
          </Button>
        }
      />
      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-xl border bg-card p-5">
          <h3 className="font-mono text-[16px] font-semibold text-bright">read_uncommitted</h3>
          <p className="mt-1 text-[13px] text-muted-foreground">The default of most clients.</p>
          <div className="mt-3">
            <Kv rows={[["Reads up to", <span className="font-mono">{number(end - 1)}</span>], ["Sees", `every record, including the ${here} of an undecided transaction`]]} />
          </div>
        </section>
        <section className="rounded-xl border bg-card p-5">
          <h3 className="font-mono text-[16px] font-semibold text-bright">read_committed</h3>
          <p className="mt-1 text-[13px] text-muted-foreground">Exactly-once consumers.</p>
          <div className="mt-3">
            <Kv rows={[["Reads up to", <span className="font-mono">{number(stable - 1)}</span>], ["Waits for", `the transaction to commit (then sees ${here} more) or abort (then skips them)`]]} />
          </div>
        </section>
      </div>
      <section className="mt-8">
        <h2 className="text-[17px] font-semibold text-bright">
          What a Fetch answers on {v.topic}-{v.tail_partition}
        </h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">The last stable offset in a Fetch answer is the real one at every isolation level.</p>
        <div className="mt-3 overflow-x-auto rounded-xl border bg-card">
          <table className="w-full text-[13px]">
            <thead className="bg-well text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
              <tr>
                <th className="h-10 px-4 font-medium">Field</th>
                <th className="px-4 font-medium">Value</th>
              </tr>
            </thead>
            <tbody className="font-mono">
              <tr className="h-10 border-t">
                <td className="px-4">high_watermark</td>
                <td className="px-4">{number(end)}</td>
              </tr>
              <tr className="h-10 border-t">
                <td className="px-4">last_stable_offset</td>
                <td className="px-4">{number(stable)}</td>
              </tr>
              <tr className="h-10 border-t">
                <td className="px-4">ListOffsets latest, read_committed</td>
                <td className="px-4">{number(stable)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
      <div className="mt-6 flex gap-3 rounded-xl border bg-card p-4 text-[13.5px]">
        <InfoIcon className="mt-0.5 size-4.5 shrink-0 text-cool" aria-hidden />
        <p>
          Which group's fetches are read_committed is a property of each Fetch, not of the group. The wire shows it per request;{" "}
          <Link to="/live" className="font-medium text-primary hover:underline">
            see the wire
          </Link>
          .
        </p>
      </div>
    </div>
  );
}
