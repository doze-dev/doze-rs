// A stream's Data Viewer tab (kinesis__stream-data-viewer, stream-data-viewer-none, record-detail): one shard from a starting position without a consumer
// application. Get records makes GetShardIterator and GetRecords; Next makes GetRecords on the iterator the last answer gave. A record opens in a drawer.
import { DownloadIcon, ChevronRightIcon, DatabaseIcon } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { clockMsUtc, dateTimeUtc } from "@doze/ui/lib/format";
import { Segmented, RuleHelp } from "../../../kit";
import { callOp } from "../deep/api";
import { on } from "./api";
import { type KRecord, POSITIONS, type Position, arrivalMs, dataText, iteratorInput, prettyJson, shortSequence, type Stream } from "./model";

type Shards = { Shards?: { ShardId: string }[] };
type Got = { Records?: KRecord[]; NextShardIterator?: string };
/** Records a read asks for, as the board says: up to 50 at a time. */
const LIMIT = 50;
const stamp = (ms: number) => `${dateTimeUtc(ms).slice(0, 10)} ${clockMsUtc(ms)} UTC`;

export function ViewerTab({ s, oldest }: { s: Stream; oldest: { shard: string; at: number } | null }) {
  const shards = useQuery({ queryKey: ["resources", "deep", "kinesis", "streams", s.name, "shards"], queryFn: () => callOp<Shards>(on(s.name), "ListShards", {}), retry: false });
  const open = (shards.data?.Shards ?? []).map((x) => x.ShardId);
  const [shard, setShard] = useState<string | null>(null);
  const [position, setPosition] = useState<Position>("TRIM_HORIZON");
  const [sequence, setSequence] = useState("");
  const [at, setAt] = useState<number | null>(() => Date.now() - 3_600_000);
  const [got, setGot] = useState<{ shard: string; records: KRecord[]; next: string | null } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [looking, setLooking] = useState<KRecord | null>(null);
  const chosen = shard ?? open[0] ?? "";
  const run = async (next: string | null) => {
    setBusy(true);
    setError(null);
    try {
      const iterator = next ?? (await callOp<{ ShardIterator: string }>(on(s.name), "GetShardIterator", iteratorInput(chosen, position, { sequence, at }))).ShardIterator;
      const r = await callOp<Got>(on(s.name), "GetRecords", { ShardIterator: iterator, Limit: LIMIT });
      setGot({ shard: chosen, records: r.Records ?? [], next: r.NextShardIterator ?? null });
    } catch (e) {
      // A failed first read leaves nothing to show: the error stands alone, not above the last answer's records. A failed Next keeps what was read.
      if (next === null) setGot(null);
      setError(e);
    } finally {
      setBusy(false);
    }
  };
  const columns: DataColumn<KRecord>[] = [
    { id: "seq", header: "Sequence number", size: 190, accessorFn: (r) => r.SequenceNumber, meta: { mono: true }, cell: (c) => <button type="button" onClick={() => setLooking(c.row.original)} className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={c.row.original.SequenceNumber} data-testid="record-open">{shortSequence(c.row.original.SequenceNumber)}</button> },
    { id: "key", header: "Partition key", size: 150, accessorFn: (r) => r.PartitionKey, meta: { mono: true } },
    { id: "at", header: "Approximate arrival timestamp", size: 250, accessorFn: (r) => r.ApproximateArrivalTimestamp ?? 0, cell: (c) => { const ms = arrivalMs(c.row.original); return ms === null ? "–" : <span className="tabular-nums">{stamp(ms)}</span>; } },
    { id: "data", header: "Data", accessorFn: (r) => dataText(r.Data) ?? "", meta: { mono: true }, cell: (c) => <span className="block max-w-[22rem] truncate font-mono text-[13px]" title={dataText(c.row.original.Data) ?? "(binary)"}>{dataText(c.row.original.Data) ?? "(binary data)"}</span> },
  ];
  const failed = error ? errorWords(error) : null;
  return (
    <div className="grid gap-8" data-testid="stream-viewer">
      <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Shard and starting position">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Shard and starting position</h2>
        <p className="mt-1 text-[13.5px] text-muted-foreground">Shows the records in one shard without a consumer application.</p>
        <div className="mt-5 grid gap-5 md:grid-cols-2">
          <div className="grid content-start gap-1.5">
            <Label htmlFor="viewer-shard">Shard</Label>
            <Select value={chosen} onValueChange={setShard}><SelectTrigger id="viewer-shard" className="w-full bg-card font-mono" data-testid="viewer-shard"><SelectValue placeholder="Reading shards…" /></SelectTrigger><SelectContent>{open.map((id) => <SelectItem key={id} value={id} className="font-mono">{id}</SelectItem>)}</SelectContent></Select>
          </div>
          <div className="grid content-start gap-1.5">
            <Label htmlFor="viewer-position">Starting Position</Label>
            <Select value={position} onValueChange={(v) => setPosition(v as Position)}><SelectTrigger id="viewer-position" className="w-full bg-card" data-testid="viewer-position"><SelectValue /></SelectTrigger><SelectContent>{POSITIONS.map((p) => <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>)}</SelectContent></Select>
            <p className="text-[13px] text-muted-foreground">Also: {POSITIONS.filter((p) => p.value !== position && p.value !== "TRIM_HORIZON").map((p) => p.label).join(", ")}.</p>
          </div>
          {(position === "AT_SEQUENCE_NUMBER" || position === "AFTER_SEQUENCE_NUMBER") && (
            <div className="grid gap-1.5 md:col-span-2"><Label htmlFor="viewer-sequence">Sequence number</Label><Input id="viewer-sequence" value={sequence} onChange={(e) => setSequence(e.target.value)} placeholder="4900000000…" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="viewer-sequence" /></div>
          )}
          {position === "AT_TIMESTAMP" && (
            <div className="grid gap-1.5 md:col-span-2"><Label>Timestamp, UTC</Label><UtcDateTimePicker value={at} onChange={setAt} aria-label="Timestamp, UTC" className="max-w-[20rem]" /></div>
          )}
        </div>
        <div className="mt-6"><Button type="button" disabled={busy || chosen === ""} onClick={() => void run(null)} data-testid="viewer-get"><DownloadIcon /> {busy ? "Reading…" : "Get records"}</Button></div>
        <RuleHelp className="mt-6 max-w-[46rem]"><b>Trim horizon</b> starts at the last untrimmed record, the oldest in the shard{oldest ? <>, here {dateTimeUtc(oldest.at)}</> : null} because {s.name} keeps {s.retention} hours. <b>Latest</b> starts just after the most recent record. <span className="text-muted-foreground">Each Get records or Next is a GetRecords call; the iterator behind it expires after 5 minutes.</span></RuleHelp>
      </section>
      {failed && <Alert tone="bad" title={`The records could not be read${failed.code ? `: ${failed.code}` : ""}`} testId="viewer-error">{failed.message}</Alert>}
      {got && (
        <section aria-label="Records" data-testid="viewer-records">
          <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
            <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Records</h2>
            <p className="max-w-[34rem] text-right text-[13px] text-muted-foreground" data-testid="viewer-count">{got.records.length} {got.records.length === 1 ? "record" : "records"} from {got.shard}{got.records.length > 0 ? `, up to ${LIMIT} shown at a time. The columns are the fields of each Record that GetRecords returns.` : "."}</p>
          </div>
          {got.records.length === 0 ? (
            <EmptyTab icon={DatabaseIcon} title="No records found" testId="viewer-empty">GetRecords returned nothing from this position. Records appear here once a producer writes to the shard.</EmptyTab>
          ) : (
            <>
              <DataTable aria-label="Records" data={got.records} columns={columns} rowId={(r) => r.SequenceNumber} maxHeight={560} />
              <div className="mt-4 flex items-center justify-end gap-3 text-[13px] text-muted-foreground">
                <span data-testid="viewer-showing">Showing {got.records.length} {got.records.length === 1 ? "record" : "records"}</span>
                <Button type="button" variant="outline" size="sm" className="bg-card" disabled={busy || got.next === null} onClick={() => void run(got.next)} data-testid="viewer-next">Next <ChevronRightIcon /></Button>
              </div>
            </>
          )}
        </section>
      )}
      <RecordDialog key={looking?.SequenceNumber ?? ""} r={looking} onClose={() => setLooking(null)} />
    </div>
  );
}

/** One record (kinesis__record-detail): its payload as raw data or as JSON, and the fields of the Record beside it. */
function RecordDialog({ r, onClose }: { r: KRecord | null; onClose: () => void }) {
  const [picked, setView] = useState<"raw" | "json" | null>(null);
  const text = r ? dataText(r.Data) : null;
  const json = text === null ? null : prettyJson(text);
  // The board opens on JSON; data that is not JSON opens as it is, and Raw data is a click away either way.
  const view = picked ?? (json !== null ? "json" : "raw");
  const ms = r ? arrivalMs(r) : null;
  return (
    <Dialog open={r !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] gap-0 overflow-y-auto sm:max-w-[600px]" data-testid="record-drawer">
        {r && (
          <>
            <DialogHeader className="pb-5">
              <DialogTitle className="text-[21px] font-semibold">Record</DialogTitle>
              <DialogDescription className="font-mono text-[12.5px] break-all">{r.PartitionKey}</DialogDescription>
            </DialogHeader>
            <div className="grid gap-6">
              <section className="grid gap-3">
                <div className="flex items-center justify-between gap-3"><h3 className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Payload</h3><div className="flex items-center gap-2"><Segmented label="Payload view" testId="record-view" value={view} onChange={setView} options={[{ value: "raw", label: "Raw data" }, { value: "json", label: "JSON" }]} />{text !== null && <CopyButton text={text} what="Data copied" className="bg-card" />}</div></div>
                {text === null ? <p className="text-[13.5px] text-muted-foreground" data-testid="record-binary">The data is not text, so it cannot be shown. It is {r.Data?.length ?? 0} characters of base64.</p> : view === "json" && json === null ? <p className="text-[13.5px] text-muted-foreground" data-testid="record-not-json">The data is not JSON, so it is shown as it is.</p> : null}
                {text !== null && <CodeBlock text={view === "json" && json !== null ? json : text} wrap max="max-h-80" testId="record-data" />}
              </section>
              <section className="grid gap-1" data-testid="record-details">
                <h3 className="mb-1 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Details</h3>
                <dl className="grid grid-cols-[minmax(9rem,12rem)_minmax(0,1fr)] text-[13.5px] [&>div:last-child>*]:border-b-0">
                  {([["Sequence number", <span key="s" className="font-mono break-all">{r.SequenceNumber}</span>], ["Partition key", <span key="p" className="font-mono">{r.PartitionKey}</span>], ["Approximate arrival timestamp", ms === null ? "–" : stamp(ms)], ["Encryption type", r.EncryptionType ?? "NONE"]] as [string, React.ReactNode][]).map(([l, v]) => (
                    <div key={l} className="contents"><dt className="border-b py-2.5 pr-4 text-muted-foreground">{l}</dt><dd className="border-b py-2.5 text-bright">{v}</dd></div>
                  ))}
                </dl>
              </section>
              <div className="flex justify-end border-t pt-5"><Button onClick={onClose} data-testid="record-done">Done</Button></div>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
