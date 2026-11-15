// One record, completely (boards topic-record, topic-record-binary; classic pages/topics/record.rs): a Sheet over the
// Messages page, routed by `?record=partition:offset` (a deep link opens the page with the record; Back closes it).
import { InfoIcon, TriangleAlertIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { ApiFailure } from "@/api/client";
import { useRecord } from "@/api/topics";
import type { RecordView } from "@/api/types/RecordView";
import type { Shown } from "@/api/types/Shown";
import { CopyButton } from "@/components/app/copy-button";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { bytes, clock, clockMs, timeoutText } from "@/lib/kafka";
import { STANDING } from "./words";

const KIND: Record<Shown["kind"], string> = { null: "null", empty: "empty", text: "text", json: "JSON", binary: "binary" };

function note(s: Shown): string {
  if (s.kind === "null") return "null";
  return `${s.bytes === 1 ? "1 byte" : `${number(s.bytes)} bytes`} · ${KIND[s.kind]}`;
}

/** JSON as it was sent, or laid out; anything that does not parse stays as it was. */
export function pretty(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function Field({ label, aside, children, action }: { label: string; aside: string; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex min-h-8 items-center justify-between gap-3">
        <h3 className="text-[13px] font-semibold text-bright">
          {label} <span className="ml-1.5 font-normal text-muted-foreground">{aside}</span>
        </h3>
        <div className="flex items-center gap-2">{action}</div>
      </div>
      {children}
    </section>
  );
}

function Block({ s, testid, prettyOn }: { s: Shown; testid: string; prettyOn?: boolean }) {
  if (s.kind === "null") return <p className="text-[13px] text-muted-foreground">No value: a deletion (a tombstone).</p>;
  if (s.kind === "empty") return <p className="text-[13px] text-muted-foreground">Zero bytes.</p>;
  const text = s.kind === "json" && prettyOn ? pretty(s.text) : s.text;
  return (
    <>
      <pre data-testid={testid} className="max-h-[22rem] overflow-auto rounded-lg border bg-well px-3.5 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap break-all text-bright">
        {text}
      </pre>
      {s.clipped && (
        <p className="text-[12.5px] text-muted-foreground">
          Shown up to {bytes(s.text.length)} of {bytes(s.bytes)}; the rest is not shown.
        </p>
      )}
    </>
  );
}

/** A key or value in the registry's wire format: what it reads as with its schema (laid out, with Copy), or why it does not, and
 * the bytes as they are a toggle away. */
function Decoded({ s, label, testid }: { s: Shown; label: string; testid: string }) {
  const [raw, setRaw] = useState(false);
  const k = s.schema!;
  const named = k.subject ? (
    <Link to="/schemas/subjects/$subject" params={{ subject: k.subject }} search={{ version: k.version ?? undefined }} className="font-mono text-link hover:underline" data-testid={`${testid}-subject`}>
      {k.subject} v{k.version}
    </Link>
  ) : null;
  return (
    <Field
      label={label}
      aside={`${note(s)} · ${k.kind ?? "schema"} id ${k.id}`}
      action={
        <>
          <ToggleGroup type="single" variant="outline" size="sm" value={raw ? "raw" : "decoded"} onValueChange={(x) => x && setRaw(x === "raw")} aria-label={`How the ${label.toLowerCase()} is shown`} data-testid={`${testid}-view`}>
            <ToggleGroupItem value="decoded" className="px-2.5 text-[12.5px]">
              Decoded
            </ToggleGroupItem>
            <ToggleGroupItem value="raw" className="px-2.5 text-[12.5px]">
              Bytes
            </ToggleGroupItem>
          </ToggleGroup>
          {!raw && k.json && <CopyButton text={k.json} what={`${label} copied`} label={`Copy ${label.toLowerCase()}`} />}
        </>
      }
    >
      {raw ? (
        <Block s={s} testid={testid} />
      ) : (
        <>
          {k.json && (<p className="text-[12.5px] text-muted-foreground">
            {k.json ? <>Decoded with {named ?? `schema id ${k.id}`}{k.message && <> as <code className="font-mono">{k.message}</code></>}: magic byte 0, the id in four bytes, then the {k.kind === "PROTOBUF" ? "message index and the Protobuf body" : k.kind === "JSON" ? "JSON text" : "Avro binary"}.</> : null}
          </p>)}
          {k.json && (
            <pre data-testid={testid} className="max-h-[22rem] overflow-auto rounded-lg border bg-well px-3.5 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap break-all text-bright">
              {k.json}
            </pre>
          )}
          {k.error && (
            <Callout tone="warn">
              <b className="font-semibold text-bright">{k.kind ? "Not decoded: the bytes do not fit their schema." : "Not decoded."}</b> <span data-testid={`${testid}-why`}>{k.error}</span>{named && <> Its schema: {named}.</>}
            </Callout>
          )}
          {k.mismatch && (
            <Callout tone="warn">
              <b className="font-semibold text-bright">It reads, but does not fit its JSON Schema.</b> <span data-testid={`${testid}-why`}>{k.mismatch}</span>
            </Callout>
          )}
        </>
      )}
    </Field>
  );
}

function StandingCallout({ v }: { v: RecordView }) {
  const lso = number(v.last_stable);
  if (v.standing === "above_lso") {
    return (
      <Callout tone="warn">
        <b className="font-semibold text-bright">At or above the last stable offset.</b> The LSO of {v.topic}-{v.partition} is {lso}
        {v.transactional && <> and this record is in {v.txn?.id ? `transaction ${v.txn.id}` : "a transaction"}, still open</>}: a read_committed consumer cannot see it yet; a read_uncommitted one can.
      </Callout>
    );
  }
  if (v.standing === "aborted") {
    return (
      <Callout tone="warn">
        <b className="font-semibold text-bright">Its transaction was aborted.</b> A read_uncommitted consumer reads it; a read_committed one skips it.
      </Callout>
    );
  }
  if (v.value.kind === "binary" && !v.value.schema) {
    return (
      <Callout tone="info">
        <b className="font-semibold text-bright">The value is not text.</b> {number(v.value.bytes)} bytes that are not valid UTF-8, shown as hex. They do not start with the schema registry's wire header (a zero byte, then a schema id), so there is no schema to read them with.
      </Callout>
    );
  }
  return null;
}

function Callout({ tone, children }: { tone: "warn" | "info"; children: React.ReactNode }) {
  const Icon = tone === "warn" ? TriangleAlertIcon : InfoIcon;
  return (
    <div className={tone === "warn" ? "flex gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4 text-[13px]" : "flex gap-3 rounded-xl border bg-card p-4 text-[13px]"} role="status">
      <Icon className={tone === "warn" ? "mt-0.5 size-4 shrink-0 text-warn" : "mt-0.5 size-4 shrink-0 text-cool"} aria-hidden />
      <p className="text-foreground">{children}</p>
    </div>
  );
}

function Facts({ v }: { v: RecordView }) {
  const row = (dt: string, dd: React.ReactNode) => (
    <div className="grid grid-cols-[8.5rem_minmax(0,1fr)] gap-3 border-b py-2.5 last:border-b-0">
      <dt className="text-[13px] text-muted-foreground">{dt}</dt>
      <dd className="min-w-0 text-[13px] text-foreground">{dd}</dd>
    </div>
  );
  const sub = (t: string) => <span className="block text-[12.5px] text-muted-foreground">{t}</span>;
  const st = STANDING[v.standing];
  return (
    <dl className="rounded-xl border bg-card px-4">
      {row(
        "Timestamp",
        <>
          <span className="font-mono">{clockMs(v.timestamp_ms)} UTC</span>
          {sub(v.log_append_time ? "LogAppendTime, set by the broker." : "CreateTime, set by the producer.")}
        </>,
      )}
      {row("Codec", v.codec)}
      {row(
        "Producer",
        v.producer ? (
          <span className="font-mono">
            {v.producer[0]} · epoch {v.producer[1]}
            {v.producer[2] >= 0 && ` · sequence ${number(v.producer[2])}`}
          </span>
        ) : (
          <>
            none {sub("a plain producer: no producer id, so no epoch or sequence")}
          </>
        ),
      )}
      {row(
        "Transaction",
        v.txn ? (
          <>
            <span className="inline-flex flex-wrap items-center gap-2">
              {v.txn.id ? <span className="font-mono">{v.txn.id}</span> : "unknown transaction"}
              {v.txn.state && <StatusDot tone={v.txn.state === "Ongoing" ? "warn" : "info"}>{v.txn.state}</StatusDot>}
            </span>
            {v.txn.since_ms !== null && v.txn.timeout_ms !== null ? sub(`since ${clock(v.txn.since_ms)}, timeout ${timeoutText(v.txn.timeout_ms)}`) : sub("the coordinator no longer has it open")}
          </>
        ) : (
          "none"
        ),
      )}
      {!v.txn &&
        row(
          "Position",
          <>
            <StatusDot tone={st.tone}>{st.word}</StatusDot>
            {sub(
              v.standing === "committed"
                ? `below the LSO of ${v.topic}-${v.partition} (${number(v.last_stable)}): both isolation levels see it`
                : v.standing === "above_lso"
                  ? `at or above the LSO of ${v.topic}-${v.partition} (${number(v.last_stable)})`
                  : "below the LSO, in a transaction that was aborted",
            )}
          </>,
        )}
    </dl>
  );
}

function Body({ v }: { v: RecordView }) {
  const [view, setView] = useState<"pretty" | "raw">("pretty");
  const binary = v.value.kind === "binary";
  return (
    <div className="flex flex-col gap-5 px-6 pb-8" data-testid="record-body">
      <StandingCallout v={v} />
      {v.key.schema ? (
        <Decoded s={v.key} label="Key" testid="record-key" />
      ) : (
      <Field label="Key" aside={note(v.key)} action={v.key.kind !== "null" && v.key.kind !== "empty" && <CopyButton text={v.key.text} what="Key copied" label="Copy key" iconOnly variant="ghost" />}>
        <Block s={v.key} testid="record-key" />
      </Field>
      )}
      {v.value.schema ? (
        <Decoded s={v.value} label="Value" testid="record-value" />
      ) : (
      <Field
        label="Value"
        aside={note(v.value)}
        action={
          <>
            {v.value.kind === "json" && (
              <ToggleGroup type="single" variant="outline" size="sm" value={view} onValueChange={(x) => x && setView(x as "pretty" | "raw")} aria-label="How the JSON is shown" data-testid="json-view">
                <ToggleGroupItem value="pretty" className="px-2.5 text-[12.5px]">
                  Pretty
                </ToggleGroupItem>
                <ToggleGroupItem value="raw" className="px-2.5 text-[12.5px]">
                  As sent
                </ToggleGroupItem>
              </ToggleGroup>
            )}
            {v.value.kind !== "null" && v.value.kind !== "empty" && <CopyButton text={v.value.kind === "json" && view === "pretty" ? pretty(v.value.text) : v.value.text} what={binary ? "Hex copied" : "Value copied"} label={binary ? "Copy as hex" : "Copy value"} />}
          </>
        }
      >
        <Block s={v.value} testid="record-value" prettyOn={view === "pretty"} />
      </Field>
      )}
      <Field label="Headers" aside={v.headers.length ? String(v.headers.length) : "none"}>
        {v.headers.length === 0 ? (
          <p className="text-[13px] text-muted-foreground">No headers were sent with this record.</p>
        ) : (
          <dl className="rounded-xl border bg-card px-4" data-testid="record-headers">
            {v.headers.map((h, i) => (
              <div key={`${h.key}-${i}`} className="grid grid-cols-[minmax(0,12rem)_minmax(0,1fr)] gap-3 border-b py-2 font-mono text-[12.5px] last:border-b-0">
                <dt className="truncate text-muted-foreground" title={h.key}>
                  {h.key}
                </dt>
                <dd className="break-all text-bright">{h.value.kind === "binary" ? h.value.preview : h.value.kind === "null" ? "null" : h.value.text}</dd>
              </div>
            ))}
          </dl>
        )}
      </Field>
      <Facts v={v} />
    </div>
  );
}

/** The Sheet. `at` is the record's place, or null when none is asked for. */
export function RecordSheet({ topic, at, onClose }: { topic: string; at: { partition: number; offset: number } | null; onClose: () => void }) {
  const r = useRecord(topic, at);
  const missing = r.error instanceof ApiFailure ? r.error.message : r.error ? "The record could not be loaded." : null;
  return (
    <Sheet open={!!at} onOpenChange={(o) => !o && onClose()}>
      <SheetContent
        className="w-full gap-0 overflow-y-auto sm:max-w-2xl"
        data-testid="record-sheet"
      >
        <SheetHeader className="px-6 pt-6 pb-4">
          <SheetTitle className="font-mono text-[17px] break-all">{at ? `${topic}-${at.partition} · ${number(at.offset)}` : topic}</SheetTitle>
          <SheetDescription>One record, completely.</SheetDescription>
        </SheetHeader>
        {r.data ? (
          <Body v={r.data} />
        ) : missing ? (
          <div className="flex flex-col gap-4 px-6">
            <p className="text-[13.5px] text-foreground" data-testid="record-missing">
              {missing}
            </p>
            <Button variant="outline" className="w-fit" onClick={onClose}>
              Back to the messages
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-4 px-6">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-40 w-full" />
            <Skeleton className="h-24 w-full" />
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}
