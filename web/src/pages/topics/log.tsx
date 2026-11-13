// A partition's Log (boards topic-log, log-planner, log-retention, log-empty, log-compact-now; classic pages/log/*): the
// ruler of offsets with the gaps compaction left, the planner's or retention's rules with this partition's numbers, the last
// round or pass, the deletions kept, durability, the segments, the other logs; and the Lab's Compact now in a Dialog.
import { Link, useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { CheckIcon, ChevronDownIcon, EyeIcon, FlaskConicalIcon, InfoIcon, ScrollTextIcon, SendIcon, TriangleAlertIcon } from "lucide-react";
import { useRef, useState } from "react";
import { toast } from "sonner";
import { ApiFailure } from "@/api/client";
import { useCompactNow, useLog } from "@/api/topics";
import type { LogCompaction } from "@/api/types/LogCompaction";
import type { LogResponse } from "@/api/types/LogResponse";
import type { LogRule } from "@/api/types/LogRule";
import type { LogSegment } from "@/api/types/LogSegment";
import type { LogStop } from "@/api/types/LogStop";
import type { LogView } from "@/api/types/LogView";
import type { RulerCellKind } from "@/api/types/RulerCellKind";
import type { RulerKey } from "@/api/types/RulerKey";
import { CopyButton } from "@/components/app/copy-button";
import { EmptyState } from "@/components/app/empty-state";
import { Spans } from "@/components/app/spans";
import { type Stat, StatStrip } from "@/components/app/stat";
import { StatusDot, type Tone } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { bytes, clock, dateTime, millis, pct, spanMs } from "@/lib/kafka";
import { useShell } from "@/api/queries";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";
import { useTopicDialog } from "./frame";
import { policyWord } from "./words";

const STATE_WORD: Record<LogResponse["state"], string> = { empty: "Empty", due: "Due", not_due: "Not due", retention: "Retention" };

const CELL: Record<RulerCellKind, { bg: string; border: string }> = {
  clean: { bg: "var(--tint-good)", border: "var(--tint-good-line)" },
  dirty: { bg: "var(--tint-warn)", border: "var(--tint-warn-line)" },
  active: { bg: "var(--tint-brand)", border: "var(--tint-brand-line)" },
  sealed: { bg: "var(--tint-cool)", border: "var(--tint-cool-line)" },
};
const HATCH = "repeating-linear-gradient(135deg,transparent 0 3px,var(--hatch) 3px 4px)";
const KEY: Record<RulerKey, { swatch: React.CSSProperties; text: string }> = {
  cleaned: { swatch: { background: CELL.clean.bg, borderColor: CELL.clean.border }, text: "cleaned: the latest record of each key" },
  dirty: { swatch: { background: CELL.dirty.bg, borderColor: CELL.dirty.border }, text: "dirty: not cleaned yet" },
  active: { swatch: { background: CELL.active.bg, borderColor: CELL.active.border }, text: "active" },
  active_never: { swatch: { background: CELL.active.bg, borderColor: CELL.active.border }, text: "active: never cleaned" },
  sealed: { swatch: { background: CELL.sealed.bg, borderColor: CELL.sealed.border }, text: "sealed" },
  hatching: { swatch: { background: HATCH }, text: "hatching: records compaction removed" },
};
const GAP = 3;

/** Marks at the same place (the log start and the checkpoint at 0) are one mark with both names, so they never overlap. */
function merged(marks: LogResponse["ruler"]["marks"]) {
  const out: LogResponse["ruler"]["marks"] = [];
  for (const m of marks) {
    const same = out.find((o) => o.offset === m.offset || Math.abs(o.at - m.at) < 0.04);
    if (same && same.offset === m.offset) {
      same.label = `${same.label} · ${m.label}`;
      same.end = same.end || m.end;
    } else out.push({ ...m });
  }
  return out;
}

function Card({ title, word, lede, children, className, ...rest }: { title: string; word?: React.ReactNode; lede?: React.ReactNode; children?: React.ReactNode; className?: string } & React.HTMLAttributes<HTMLElement>) {
  return (
    <section className={cn("min-w-0 rounded-xl border bg-card p-5", className)} {...rest}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-[16px] font-semibold text-bright">{title}</h3>
        {word}
      </div>
      {lede && <p className="mt-1 text-[13px] leading-5 text-muted-foreground">{lede}</p>}
      {children && <div className="mt-4">{children}</div>}
    </section>
  );
}

function Kv({ rows }: { rows: [string, React.ReactNode, string?][] }) {
  return (
    <dl>
      {rows.map(([dt, dd, sub]) => (
        <div key={dt} className="grid grid-cols-[9.5rem_minmax(0,1fr)] gap-3 border-b py-2.5 text-[13px] last:border-b-0">
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

function Rules({ rules }: { rules: LogRule[] }) {
  return (
    <ul className="flex flex-col" data-testid="rules">
      {rules.map((r) => (
        <li key={r.title} className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-start gap-3 border-b py-3 text-[13.5px] last:border-b-0" data-ok={r.tone === "ok"}>
          {r.tone === "ok" ? <CheckIcon className="mt-0.5 size-4 text-good" aria-label="passes" /> : <TriangleAlertIcon className="mt-0.5 size-4 text-warn" aria-label="stops it" />}
          <div className="min-w-0">
            <span className="text-bright">{r.title}</span>
            <span className="mt-0.5 block text-[12.5px] text-muted-foreground">
              <Spans spans={r.sub} />
            </span>
          </div>
          <span className="font-mono text-[12.5px] whitespace-nowrap text-foreground">{r.value}</span>
        </li>
      ))}
    </ul>
  );
}

/** The ruler: segments in offset order, as wide as the offsets they span, hatched where compaction removed records. */
function RulerView({ r }: { r: LogResponse }) {
  const ruler = r.ruler;
  const v = r.view;
  const gaps = GAP * Math.max(0, ruler.cells.length - 1);
  const firstGap = v.segments.flatMap((s) => s.gaps).find(([f, n]) => f >= v.start && f + n < v.end);
  const empty = r.state === "empty";
  return (
    <section className="mt-8" data-testid="ruler">
      {!empty && (
        <>
          <h2 className="text-[17px] font-semibold text-bright">The log, offset by offset</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">
            Every segment of partition {v.partition} in offset order, as {v.compaction ? "the cleaner" : "retention"} sees it. Open a row of the segments table below for its files and index.
          </p>
        </>
      )}
      <div className="mt-4 rounded-xl border bg-card p-5">
        <div className="relative h-14">
          {merged(ruler.marks).map((m) => {
            const style: React.CSSProperties =
              m.at <= 0 ? { left: 0, alignItems: "flex-start" } : m.at >= 1 ? { right: 0, alignItems: "flex-end" } : { left: `calc((100% - ${gaps}px) * ${m.at.toFixed(5)} + ${GAP / 2}px)`, transform: "translateX(-50%)", alignItems: "center" };
            return (
              <div key={`${m.label}-${m.offset}`} className="absolute top-0 flex flex-col text-[12px] whitespace-nowrap" style={style}>
                <span className="text-muted-foreground">{m.label}</span>
                <b className={cn("font-mono font-medium text-bright", m.end && "text-link")}>{number(m.offset)}</b>
              </div>
            );
          })}
        </div>
        <div className="flex h-20 gap-[3px]">
          {ruler.cells.map((c, i) => (
            <div key={i} className="relative flex min-w-0 items-end overflow-hidden rounded-md border px-2 pb-1.5 font-mono text-[11px] leading-4 text-foreground" style={{ flex: c.flex, background: CELL[c.cell].bg, borderColor: CELL[c.cell].border }}>
              {c.gaps.map(([at, w], j) => (
                <i key={j} className="absolute inset-y-0" style={{ left: `${at}%`, width: `${w}%`, background: HATCH }} />
              ))}
              {c.label && (
                <span className="relative truncate">
                  {c.label[0]}
                  <br />
                  {c.label[1]}
                </span>
              )}
            </div>
          ))}
        </div>
        <div className="mt-1.5 flex gap-[3px] font-mono text-[11.5px] text-muted-foreground">
          {ruler.cells.map((c, i) => (
            <span key={i} className={cn("min-w-0 truncate", i + 1 === ruler.cells.length && c.cell === "active" && "text-right")} style={{ flex: c.flex }}>
              {c.axis ?? ""}
            </span>
          ))}
        </div>
        <div className="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-[12.5px] text-muted-foreground">
          {ruler.key.map((k) => {
            const item = k === "active_never" && r.state === "retention" ? KEY.active : KEY[k];
            return (
              <span key={k} className="inline-flex items-center gap-1.5">
                <i className="inline-block size-3 rounded-sm border" style={item.swatch} />
                {item.text}
              </span>
            );
          })}
          <span>{empty ? "Nothing to compact, nothing to delete" : "Width is offsets, not bytes"}</span>
        </div>
        {!empty && v.compaction && (
          <p className="mt-4 text-[13px] text-muted-foreground">
            <b className="font-semibold text-foreground">Gaps.</b> Hatched: offsets that cleaning removed. Offsets never move, so a read from inside one carries on at the next record
            {firstGap && (
              <>
                : a Fetch at <code className="font-mono">{number(firstGap[0])}</code> returns the record at <code className="font-mono">{number(firstGap[0] + firstGap[1])}</code>, with no error
              </>
            )}
            . A cleaned batch keeps the offsets it spans, so the last offset of a segment is never lost.
          </p>
        )}
        {!empty && (
          <p className="mt-2 text-[13px] text-muted-foreground">
            <b className="font-semibold text-foreground">Log start.</b> Delete records moves it up, and everything below goes
            {v.cleanup === "compact" ? (
              <>
                {" "}
                (it is refused on this compacted topic: <code className="font-mono">POLICY_VIOLATION (44)</code>)
              </>
            ) : (
              " (the Delete records dialog of the topic does it)"
            )}
            . A Fetch below the start gets <code className="font-mono">OFFSET_OUT_OF_RANGE (1)</code>, and the last stable offset is never below the start.
            {v.start > 0 && (
              <>
                {" "}
                Here the log start is <code className="font-mono">{number(v.start)}</code>: the offsets below it are hatched.
              </>
            )}
          </p>
        )}
      </div>
    </section>
  );
}

function cleanerWord(c: LogCompaction): React.ReactNode {
  switch (c.cleaner.kind) {
    case "disabled":
      return <StatusDot tone="warn">Cleaner off</StatusDot>;
    case "running":
      return <StatusDot tone="info">Cleaning now</StatusDot>;
    case "failed":
      return <StatusDot tone="bad">Last round failed</StatusDot>;
    default:
      return c.planner.due ? <StatusDot tone="info">Due at the next look</StatusDot> : <StatusDot tone="warn">Not due</StatusDot>;
  }
}

function LastRound({ r, c }: { r: LogResponse; c: LogCompaction }) {
  const v = r.view;
  const last = c.last;
  if (!last) {
    return (
      <Card title="Last round" word={<StatusDot tone="idle">None yet</StatusDot>} lede={<>No round has run on <code className="font-mono">{v.topic}-{v.partition}</code> since the broker started.</>} data-testid="last-round">
        <p className="text-[12.5px] text-muted-foreground">The broker keeps only the last round of each partition, in memory: it is forgotten when the broker stops.</p>
      </Card>
    );
  }
  const kept = last.read === 0 ? 0 : last.kept / last.read;
  const word = c.cleaner.kind === "failed" ? <StatusDot tone="bad">Failed</StatusDot> : c.cleaner.kind === "running" ? <StatusDot tone="info">Running</StatusDot> : <StatusDot tone="ok">Done</StatusDot>;
  return (
    <Card title="Last round" word={word} lede={r.round ?? undefined} data-testid="last-round">
      <div className="flex items-center gap-3">
        <span className="h-2 flex-1 overflow-hidden rounded-full bg-track">
          <span className="block h-full rounded-full bg-primary" style={{ width: `${Math.round(kept * 100)}%` }} />
        </span>
        <span className="font-mono text-[12.5px] tabular-nums">{pct(kept)} kept</span>
      </div>
      <p className="mt-2 mb-2 text-[12.5px] text-muted-foreground">
        {number(last.kept)} of {number(last.read)} records stayed: the latest of each key.
      </p>
      <Kv
        rows={[
          ["Read", <span className="font-mono">{number(last.read)} records</span>],
          ["Kept", <span className="font-mono">{number(last.kept)} ({pct(kept)})</span>],
          ["Removed", <span className="font-mono">{number(Math.max(0, last.read - last.kept))}</span>, "older values of a key that has a newer one"],
          ["Replaced", <>{last.replaced} {last.replaced === 1 ? "segment" : "segments"}</>, "written beside the log, then swapped in"],
          ["Took", <span className="font-mono">{millis(last.took_ms)}</span>],
          ["Checkpoint", <span className="font-mono">{number(last.clean_to)}</span>, "the first dirty offset after the round"],
        ]}
      />
      {c.cleaner.kind === "failed" && (
        <p className="mt-3 text-[12.5px] text-muted-foreground">
          A later round failed at {dateTime(c.cleaner.at_ms)}: {c.cleaner.message}
        </p>
      )}
      <p className="mt-3 text-[12.5px] text-muted-foreground">The broker keeps only the last round of each partition, in memory: it is forgotten when the broker stops.</p>
    </Card>
  );
}

function PlannerCards({ r, c }: { r: LogResponse; c: LogCompaction }) {
  const v = r.view;
  const due = c.planner.due;
  return (
    <div className="mt-8 grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
      <Card title={due ? "Will the cleaner take it?" : "Why it waits"} word={cleanerWord(c)} lede={due ? "The planner's rules, as Kafka's cleaner reads them (CMP-1), with this partition's numbers." : "The same rules as for a log that is due, one of them failing."} data-testid="planner">
        <Rules rules={r.rules} />
        {due && r.would_rewrite.length > 0 && (
          <p className="mt-4 border-t pt-4 text-[13.5px]">
            <Spans spans={r.would_rewrite} />
          </p>
        )}
      </Card>
      <div className="flex min-w-0 flex-col gap-4">
        {!due && r.to_come && (
          <Card title="What would make it due" lede="The share counts sealed segments only.">
            <Kv
              rows={[
                ["Needs", <span className="font-mono">dirty over {r.to_come.needs}</span>, `more than ${bytes(c.planner.clean_bytes)} already clean`],
                ["Has", <span className="font-mono">{r.to_come.has}</span>],
                ["Still to come", <Spans spans={r.to_come.coming} />],
              ]}
            />
            <p className="mt-3 border-t pt-3 text-[12.5px] text-muted-foreground">
              Lowering <code className="font-mono">min.cleanable.dirty.ratio</code> on this topic makes it due sooner.{" "}
              <Link to="/topics/$topic/config" params={{ topic: v.topic }} className="font-medium text-link hover:underline">
                See the topic's config
              </Link>
              .
            </p>
          </Card>
        )}
        {(due || c.last || c.cleaner.kind !== "failed") && <LastRound r={r} c={c} />}
        {!due && !c.last && c.cleaner.kind === "failed" && <Card title="Last round" word={<StatusDot tone="bad">Failed</StatusDot>} lede={`The last round failed at ${dateTime(c.cleaner.at_ms)}: ${c.cleaner.message}`} />}
      </div>
    </div>
  );
}

function RetentionCards({ r }: { r: LogResponse }) {
  const v = r.view;
  const ret = v.retention;
  if (!ret) return null;
  const e = r.expiry;
  const mine = (ret.last_pass?.moved ?? []).filter((m) => m.topic === v.topic);
  const looked = Array.from({ length: Math.min(v.partitions, 8) }, (_, p) => `${v.topic}-${p}`).join(", ");
  const hm = (ms: number) => clock(ms).slice(0, 5);
  let after: React.ReactNode = null;
  if (e) {
    const first = e.going_now > 0 ? e.going_now - 1 : (e.next?.[0] ?? 0);
    after = (
      <Card title={e.going_now > 1 ? `After segments 1 to ${first + 1} go` : `After segment ${first + 1} goes`} lede="The new start is written down before any file is removed.">
        <Kv
          rows={[
            ["Log start", <span className="font-mono">{number(v.start)} to {number(e.new_start)}</span>],
            [`Fetch below ${number(e.new_start)}`, "OFFSET_OUT_OF_RANGE (1)"],
            ["Kept", <span className="font-mono">{number(Math.max(0, v.end - e.new_start))} offsets</span>],
          ]}
        />
      </Card>
    );
  }
  const passWord = !ret.last_pass ? <StatusDot tone="idle">No pass yet</StatusDot> : ret.last_pass.moved.length === 0 && ret.last_pass.more === 0 ? <StatusDot tone="idle">Deleted nothing</StatusDot> : <StatusDot tone="ok">Deleted</StatusDot>;
  return (
    <div className="mt-8 grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
      <Card title="What the next pass will do" lede={<>Retention reads <code className="font-mono">retention.ms</code> and <code className="font-mono">retention.bytes</code> of the topic, else the broker's.</>} data-testid="planner">
        <Rules rules={r.retention_rules} />
      </Card>
      <div className="flex min-w-0 flex-col gap-4">
        {after}
        <Card title="Last pass" word={passWord} lede={ret.last_pass ? `${hm(ret.last_pass.at_ms)}, on ${v.topic} (${v.partitions} ${v.partitions === 1 ? "partition" : "partitions"}).` : "No pass has run since the broker started."}>
          <Kv
            rows={[
              ["Looked at", <span className="font-mono">{looked}</span>, ret.last_pass ? `${ret.last_pass.looked} partitions in all, across every topic with a retention bound` : undefined],
              [
                "Past retention",
                mine.length === 0 ? <span className="font-mono">0 segments</span> : mine.map((m) => <span key={m.partition} className="block font-mono">{m.topic}-{m.partition}: log start {number(m.from)} to {number(m.to)}</span>),
                mine.length === 0 ? "no sealed segment was past retention" : undefined,
              ],
              ["Deleted", ret.moved_total === 0 ? "Nothing since the broker started" : `${number(ret.moved_total)} log starts moved since the broker started`, ret.moved_total === 0 ? "retention keeps no record before that" : ret.moved_at_ms !== null ? `the last at ${dateTime(ret.moved_at_ms)}` : undefined],
              ["Next pass", <span className="font-mono">{ret.last_pass ? hm(ret.last_pass.at_ms + ret.every_ms) : `within ${spanMs(ret.every_ms)}`}</span>],
            ]}
          />
          <p className="mt-3 text-[12.5px] text-muted-foreground">When a pass deletes, this card lists each partition whose log start it moved. The broker keeps the last pass only, in memory.</p>
        </Card>
      </div>
    </div>
  );
}

function Deletions({ v }: { v: LogView }) {
  const c = v.compaction;
  if (!c) return null;
  const n = v.tombstones.length;
  return (
    <Card title="Deletions" lede={<>Tombstones a round has kept, with the time after which the next round removes them. A deletion stays for <code className="font-mono">delete.retention.ms</code> ({spanMs(c.rules.delete_retention_ms)}), so a slow reader still sees that the key was deleted.</>} data-testid="deletions">
      {n > 0 && (
        <table className="w-full table-fixed text-[13px]">
          <thead className="text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
            <tr>
              <th className="h-9 w-[40%] font-medium">Key</th>
              <th className="w-[26%] font-medium">Deleted</th>
              <th className="font-medium">Goes after</th>
            </tr>
          </thead>
          <tbody>
            {v.tombstones.map((t) => (
              <tr key={t.offset} className="border-t align-top">
                <td className="truncate py-2 font-mono">
                  {t.key}
                  {t.binary && <span className="text-muted-foreground"> (not text)</span>}
                  <span className="block text-[12px] text-muted-foreground">offset {number(t.offset)}</span>
                </td>
                <td className="py-2 font-mono">{dateTime(t.written_ms)}</td>
                <td className="py-2 font-mono">
                  {dateTime(t.goes_after_ms)}
                  <span className="block text-[12px] text-muted-foreground">{t.goes_after_ms > v.now_ms ? `in ${spanMs(t.goes_after_ms - v.now_ms)}` : "at the next round"}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="mt-3 text-[12.5px] text-muted-foreground">
        {n === 0 ? "No deletion is waiting to go." : `${n === 1 ? "1 tombstone is" : `${number(n)} tombstones are`} waiting to go.`} Tombstones still in the dirty part have no horizon yet, so they are not listed.
      </p>
      {n > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <Button variant="outline" size="sm" asChild>
            <Link to="/topics/$topic/messages" params={{ topic: v.topic }} search={{ record: `${v.partition}:${v.tombstones[0]!.offset}` }}>
              <EyeIcon /> Read the tombstone
            </Link>
          </Button>
          <span className="text-[12.5px] text-muted-foreground">A Fetch at offset {number(v.tombstones[0]!.offset)} returns the record with no value.</span>
        </div>
      )}
    </Card>
  );
}

const STOP: Record<LogStop, { tone: Tone; word: string; sentence: string; note: string }> = {
  clean: { tone: "ok", word: "Clean", sentence: "The last stop was clean, so this partition was opened as vouched for.", note: "every open partition was flushed and closed" },
  unclean: { tone: "warn", word: "Not clean", sentence: "The last stop did not vouch for the data, so this partition was checked when it was opened.", note: "a crash, a kill or a power cut: nothing was vouched for" },
  none: { tone: "idle", word: "None", sentence: "This is a new data directory: there was no earlier stop.", note: "there was no earlier run" },
};

function Durability({ v }: { v: LogView }) {
  const d = v.durability;
  const sealed = Math.max(0, v.segments.length - 1);
  let active = -1;
  v.segments.forEach((s, i) => s.active && (active = i));
  const stop = STOP[d.stop];
  return (
    <Card
      title="Durability"
      word={d.offline ? <StatusDot tone="bad">Offline</StatusDot> : <StatusDot tone="ok">Online</StatusDot>}
      lede={d.offline ? `A flush failed: ${d.offline}. Appends are refused until the broker restarts; reads continue.` : `An append that asks to be durable (acks=all) is acknowledged after its flush; the others are flushed within ${millis(Math.max(1, d.fsync_ms))} of the first. ${stop.sentence}`}
      data-testid="durability"
    >
      <Kv
        rows={[
          ["Active log", <span className="font-mono">segment {active >= 0 ? active + 1 : 1}</span>, `from ${number(v.segments[v.segments.length - 1]?.base ?? v.start)}; flushed when an append asks, and on a timer for the rest`],
          ["Its index, the directory", "Kept with the log", "the index as the log grows; the directory holds the segment file names"],
          ["Sealed segments", sealed === 0 ? "none yet" : <span className="font-mono">1 to {sealed}</span>, "each flushed whole, with its name, when it was sealed"],
          ["Last stop", <StatusDot tone={stop.tone}>{stop.word}</StatusDot>, stop.note],
          ["Vouched for", d.stop === "clean" ? "Yes" : "No", d.stop === "clean" ? "only the stretch after the last index entry was checked; if it were not, the whole active segment is read and every checksum verified" : "the whole active segment was read and every checksum verified when the partition was opened"],
        ]}
      />
      <p className="mt-3 text-[12.5px] text-muted-foreground">A failed flush is never retried: the partition goes offline until restart, appends are refused and reads continue.</p>
    </Card>
  );
}

function segState(v: LogView, s: LogSegment): { tone: Tone; word: string; sub: string } {
  const c = v.compaction;
  if (!c) return s.active ? { tone: "info", word: "active", sub: "deleted last, once past retention itself" } : { tone: "idle", word: "sealed", sub: "" };
  if (s.active) return { tone: "info", word: "active", sub: "never cleaned" };
  const span = Math.max(0, s.next - s.base);
  const removed = s.gaps.reduce((a, g) => a + g[1], 0) + s.unplaced;
  if (s.base < c.planner.dirty_start) return { tone: "ok", word: "cleaned", sub: removed > 0 && span > 0 ? `kept ${pct(1 - removed / span)} of its offsets` : "nothing to remove" };
  return { tone: "warn", word: "dirty", sub: s.base <= c.planner.first_dirty && c.planner.first_dirty < s.next ? "holds the checkpoint" : "" };
}

const GRID = "grid grid-cols-[7.5rem_12rem_6.5rem_9rem_minmax(0,1fr)_1.25rem] items-center gap-4";
const MOST_ROWS = 200;

function Segments({ v }: { v: LogView }) {
  const firstDirty = v.compaction ? v.segments.findIndex((s) => !s.active && s.base <= v.compaction!.planner.first_dirty && v.compaction!.planner.first_dirty < s.next) : -1;
  const [open, setOpen] = useState<Set<number>>(() => new Set(firstDirty >= 0 ? [firstDirty] : []));
  const skip = Math.max(0, v.segments.length - MOST_ROWS);
  const toggle = (i: number) => setOpen((o) => (o.has(i) ? new Set([...o].filter((x) => x !== i)) : new Set([...o, i])));
  return (
    <section className="mt-10" data-testid="segments">
      <h2 className="text-[17px] font-semibold text-bright">Segments</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">
        {v.segments.length} {v.segments.length === 1 ? "segment" : "segments"} of <code className="font-mono">segment.bytes</code> {number(v.segment_bytes)} ({bytes(v.segment_bytes)}).{" "}
        {v.compaction && "Sealed segments are cleaned at most once per round; the active one never. "}Open a row for its files and index.
        {v.partial && " The log is large: some segments were read in part, and what is missing is not guessed."}
        {skip > 0 && ` The newest ${MOST_ROWS} are listed.`}
      </p>
      <div className="mt-4 overflow-x-auto rounded-xl border bg-card">
        <div className="min-w-[46rem]">
          <div className={cn(GRID, "h-11 border-b bg-well px-4 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase")}>
            <span>Segment</span>
            <span>Offsets</span>
            <span>Size</span>
            <span>Newest record</span>
            <span>State</span>
            <span />
          </div>
          {v.segments.map((s, i) => {
            if (i < skip) return null;
            const st = segState(v, s);
            const isOpen = open.has(i);
            const newest = s.newest_ms === null ? "none" : s.active && v.now_ms - s.newest_ms < 60_000 ? "just now" : dateTime(s.newest_ms);
            const name = String(s.base).padStart(20, "0");
            const last = s.next > s.base ? s.next - 1 : s.base;
            const span = Math.max(0, s.next - s.base);
            const removed = s.gaps.reduce((a, g) => a + g[1], 0) + s.unplaced;
            return (
              <div key={s.base} className="border-b last:border-b-0" data-testid="segment">
                <button type="button" className={cn(GRID, "min-h-14 w-full px-4 py-2 text-left text-[13px] hover:bg-accent/40 focus-visible:bg-accent/40 focus-visible:outline-none")} aria-expanded={isOpen} onClick={() => toggle(i)}>
                  <span className="font-mono font-medium text-bright">Segment {i + 1}</span>
                  <span className="font-mono">
                    {number(s.base)} to {number(s.next)}
                  </span>
                  <span className="font-mono">{bytes(s.bytes)}</span>
                  <span className="truncate font-mono">{newest}</span>
                  <span className="min-w-0">
                    <StatusDot tone={st.tone}>{st.word}</StatusDot>
                    {st.sub && <span className="block truncate text-[12.5px] text-muted-foreground">{st.sub}</span>}
                  </span>
                  <ChevronDownIcon className={cn("size-4 text-muted-foreground transition-transform duration-200", !isOpen && "-rotate-90")} aria-hidden />
                </button>
                {isOpen && (
                  <div className="border-t bg-well/50 px-4 pb-3">
                    <Kv
                      rows={[
                        ["Offsets", <span className="font-mono">{number(s.base)} to {number(last)}</span>, s.active ? `${number(span)} offsets so far; the log ends at ${number(v.end)}` : `${number(span)} offsets; the next segment starts at ${number(s.next)}`],
                        ["Size", <span className="font-mono">{bytes(s.bytes)}</span>, s.active ? `of segment.bytes ${number(v.segment_bytes)}; rolls when full` : removed > 0 ? `smaller than ${bytes(v.segment_bytes)} because cleaning removed records` : `segment.bytes ${number(v.segment_bytes)}`],
                        ["Newest record", <span className="font-mono">{newest}</span>],
                        ...(v.compaction
                          ? ([["Cleaned", s.active ? "Never" : s.base < v.compaction.planner.dirty_start ? "Yes" : "Not yet", s.active ? "the active segment is left alone" : s.base < v.compaction.planner.dirty_start ? (removed > 0 && span > 0 ? `kept ${pct(1 - removed / span)} of its offsets` : "nothing was left to remove") : "no round has looked at its records"]] as [string, React.ReactNode, string][])
                          : []),
                        ["Files", <span className="font-mono">{name}.log</span>, `${name}.index`],
                        ["Index", `about ${number(Math.max(1, Math.floor(s.bytes / Math.max(1, v.index_interval_bytes))))} entries`, `one for roughly every index.interval.bytes ${number(v.index_interval_bytes)} bytes`],
                      ]}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function Others({ v }: { v: LogView }) {
  if (!v.others.length) return null;
  const button = { not_due: "Why not yet?", retention: "Retention", empty: "Empty log", plain: "Log" } as const;
  return (
    <section className="mt-10" data-testid="others">
      <h2 className="text-[17px] font-semibold text-bright">Other logs on this broker</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">The Log page of the other topics, with what is worth knowing of each.</p>
      <div className="mt-4 overflow-x-auto rounded-xl border bg-card">
        <table className="w-full min-w-[40rem] table-fixed text-[13px]">
          <colgroup>
            <col className="w-[28%]" />
            <col className="w-[16%]" />
            <col />
            <col className="w-[9.5rem]" />
          </colgroup>
          <thead className="bg-well text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
            <tr>
              <th className="h-10 px-4 font-medium">Topic</th>
              <th className="px-4 font-medium">Policy</th>
              <th className="px-4 font-medium">Its log</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {v.others.map((o) => (
              <tr key={o.topic} className="border-t">
                <td className="truncate px-4 py-2.5 font-mono">{o.topic}</td>
                <td className="px-4">{policyWord(o.cleanup)}</td>
                <td className="px-4 py-2.5">
                  {o.line}
                  {o.sub && <span className="block text-[12.5px] text-muted-foreground">{o.sub}</span>}
                </td>
                <td className="px-4 text-right">
                  <Button variant="outline" size="sm" asChild>
                    <Link to="/topics/$topic/log" params={{ topic: o.topic }}>
                      {button[o.link]}
                    </Link>
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function EmptyLog({ v }: { v: LogView }) {
  const { open } = useTopicDialog();
  const listen = useShell().data?.address ?? "";
  const cmd = `echo '{"order_id":"ord-88412"}' | kcat -P -b ${listen} -t ${v.topic}`;
  return (
    <EmptyState
      icon={ScrollTextIcon}
      title="This log has nothing in it"
      className="mt-6"
      action={
        <>
          <Button variant="outline" onClick={() => open("produce")}>
            <SendIcon /> Produce a record
          </Button>
          <CopyButton text={cmd} what="Command copied" label="Copy the command" />
        </>
      }
    >
      The first record written lands at offset {v.end}. Segments appear when the active one reaches <code className="font-mono">segment.bytes</code> ({bytes(v.segment_bytes)}){v.segment_ms !== null && ` or is ${spanMs(v.segment_ms)} old`}.
      <pre className="mt-4 overflow-x-auto rounded-lg border bg-well p-3 text-left font-mono text-[12px] text-foreground">{cmd}</pre>
    </EmptyState>
  );
}

/** The Lab's Compact now: the planner's verdict and what a round would do, a confirm, then what the broker did. */
function CompactNow({ r }: { r: LogResponse }) {
  const v = r.view;
  const d = r.compact;
  const [open, setOpen] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const run = useCompactNow(v.topic, v.partition);
  return (
    <>
      <Button
        ref={opener}
        onClick={() => {
          setRefused(null);
          setOpen(true);
        }}
        data-testid="compact-now"
      >
        <FlaskConicalIcon /> Compact now
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent
          data-testid="compact-dialog"
          onCloseAutoFocus={(e) => {
            // A clicked button is not focused in Firefox and Safari: give the focus back to the opener by hand.
            e.preventDefault();
            opener.current?.focus();
          }}
        >
          <DialogHeader>
            <DialogTitle>
              Compact <span className="font-mono">{v.topic}-{v.partition}</span> now?
            </DialogTitle>
            <DialogDescription>The cleaner's real round, run at once by the Lab, with the planner's verdict first. The same round the cleaner runs at its next look.</DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-3 text-[13.5px]">
            {d.off ? (
              <p data-testid="compact-blocked">The Lab is switched off (--lab false), so the console does not run the cleaner.</p>
            ) : d.blocked ? (
              <p className="flex gap-2.5 rounded-lg border bg-well p-3" data-testid="compact-blocked">
                <InfoIcon className="mt-0.5 size-4 shrink-0 text-cool" aria-hidden />
                {d.blocked}
              </p>
            ) : (
              <>
                <p>
                  <StatusDot tone="info">Due</StatusDot> <span className="ml-2 font-mono">{d.due}</span>
                </p>
                {d.rewrite.length > 0 && (
                  <p className="text-muted-foreground">
                    <Spans spans={d.rewrite} />
                  </p>
                )}
                <p className="text-muted-foreground">
                  The checkpoint moves from <code className="font-mono text-foreground">{number(d.checkpoint[0])}</code> to <code className="font-mono text-foreground">{number(d.checkpoint[1])}</code>.
                </p>
              </>
            )}
            {refused && (
              <p className="rounded-lg border border-refused/30 bg-refused/5 p-3 text-foreground" role="alert" data-testid="compact-refused">
                <b className="font-semibold text-refused">Refused.</b> {refused}
              </p>
            )}
          </div>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">{d.blocked || d.off ? "Close" : "Cancel"}</Button>
            </DialogClose>
            {!d.blocked && !d.off && (
              <Button
                disabled={run.isPending}
                data-testid="compact-run"
                onClick={() =>
                  run.mutate(undefined, {
                    onSuccess: (done) => {
                      setOpen(false);
                      toast.success("Compacted", { description: done.message });
                    },
                    onError: (e) => setRefused(e instanceof ApiFailure ? e.message : "The console did not answer."),
                  })
                }
              >
                Run the round
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

export function LogPage() {
  const { topic } = useParams({ from: "/topics/$topic/log" });
  const search = useSearch({ from: "/topics/$topic/log" });
  const navigate = useNavigate({ from: "/topics/$topic/log" });
  const partition = search.partition ?? 0;
  const q = useLog(topic, partition);
  if (q.error && !q.data) return <PageError error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-96 w-full rounded-xl" />;
  const r = q.data;
  const v = r.view;
  const stats: Stat[] = [
    { label: "Start offset", value: number(v.start), ch: 9 },
    { label: "End offset", value: number(v.end), ch: 9 },
    { label: "Last stable", value: number(v.last_stable), ch: 9 },
    { label: "Records", value: number(Math.max(0, v.segments.reduce((a, s) => a + s.records, 0))), ch: 9 },
    { label: "Segments", value: number(v.segments.length), ch: 4 },
    { label: "Size", value: bytes(v.segments.reduce((a, s) => a + s.bytes, 0)), ch: 9 },
  ];
  const compacts = v.cleanup !== "delete";
  const notDue = r.state === "not_due" && v.compaction;
  const ratio = v.compaction && v.compaction.planner.dirty_bytes > 0 ? v.compaction.planner.dirty_bytes / (v.compaction.planner.dirty_bytes + v.compaction.planner.clean_bytes) : null;
  return (
    <div data-testid="log" data-state={r.state}>
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <StatusDot tone={r.state === "due" ? "info" : r.state === "retention" ? "ok" : "idle"} data-testid="log-state">{STATE_WORD[r.state]}</StatusDot>
          {v.cleanup === "both" && (
            <Badge variant="outline" className="font-mono text-[11.5px]">
              compact,delete
            </Badge>
          )}
          <Badge variant="outline" className="font-mono text-[11.5px]">
            partition {v.partition} of {v.partitions}
          </Badge>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          {v.partitions > 1 && (
            <ToggleGroup type="single" variant="outline" size="sm" value={String(v.partition)} onValueChange={(x) => x && void navigate({ search: (s) => ({ ...s, partition: x === "0" ? undefined : Number(x) }), replace: true, resetScroll: false })} aria-label="Partition" className="bg-card" data-testid="log-partition">
              {Array.from({ length: Math.min(v.partitions, 64) }, (_, p) => (
                <ToggleGroupItem key={p} value={String(p)} className="min-w-9 px-2.5 font-mono">
                  {p}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          )}
          {compacts && <CompactNow r={r} />}
        </div>
      </div>
      <StatStrip stats={stats} />
      {notDue && (
        <div className="mt-6 flex gap-3 rounded-xl border bg-card p-4 text-[13.5px]">
          <InfoIcon className="mt-0.5 size-4.5 shrink-0 text-cool" aria-hidden />
          <p>
            <b className="font-semibold text-bright">{ratio !== null ? `Not yet: ${pct(ratio)} of the log is dirty, the cleaner waits for more than ${pct(v.compaction!.rules.min_dirty_ratio)}.` : "Not yet: nothing sealed is dirty."}</b> The active segment is never cleaned.
          </p>
        </div>
      )}
      <RulerView r={r} />
      {r.state === "empty" ? (
        <EmptyLog v={v} />
      ) : (
        <>
          {v.compaction ? <PlannerCards r={r} c={v.compaction} /> : <RetentionCards r={r} />}
          <div className="mt-6 grid gap-4 lg:grid-cols-2">
            <Deletions v={v} />
            <Durability v={v} />
          </div>
          <Segments v={v} />
          <Others v={v} />
        </>
      )}
    </div>
  );
}
