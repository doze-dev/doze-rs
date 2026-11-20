import { getRouteApi } from "@tanstack/react-router";
import { PauseIcon, PlayIcon, RadioIcon } from "lucide-react";
import { useMemo } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { HeldPill } from "@doze/ui/components/app/held-pill";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Figures } from "@doze/ui/components/app/stat";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { bytes, clockMsUtc, micros, number } from "@doze/ui/lib/format";
import { useTailOwnsRefresh } from "@doze/ui/lib/refresh";
import { useAbout } from "../../api/queries";
import type { CallRecord } from "../../api/types/CallRecord";
import { serviceTitle } from "../../words";
import { CallSheet } from "./call-sheet";
import { Result } from "./outcome";
import { findCall } from "./tape";
import { type Conn, useTail } from "./use-tail";

const route = getRouteApi("/live");

const dim = <span className="text-muted-foreground">–</span>;

function connWords(conn: Conn, paused: boolean): { tone: Tone; word: string } {
  if (conn.state === "waiting") return { tone: "warn", word: "Reconnecting" };
  if (conn.state === "connecting") return { tone: "info", word: "Connecting" };
  return paused ? { tone: "idle", word: "Paused" } : { tone: "ok", word: "Live" };
}

/** Live: every AWS API call this doze-aws answers, as it happens. The list is the record's calls, newest first, kept
 * current by the stream; Pause holds new calls aside; a row opens its drawer. Every column is a field of the contract. */
export function LivePage() {
  useTailOwnsRefresh();
  const { tape, conn, pause, resume, retry } = useTail();
  const about = useAbout().data;
  const { call: openSeq } = route.useSearch();
  const navigate = route.useNavigate();
  const open = (seq: number | undefined) => void navigate({ search: seq === undefined ? {} : { call: seq } });
  const titles = useMemo(() => new Map((about?.services ?? []).map((s) => [s.name, s.title])), [about]);

  const columns = useMemo<DataColumn<CallRecord>[]>(
    () => [
      // The widths add up to what a 1440 px window leaves beside the rail, with the rest for Resource; a narrower window
      // scrolls the table in its box. The time is never cut; a long service title, operation or error code is, with the
      // whole value on hover and in the drawer. The region a call was signed for is in its drawer, not a column.
      { id: "time", header: "Time", size: 126, enableSorting: false, accessorFn: (c) => clockMsUtc(c.at_ms), meta: { mono: true }, cell: (c) => <span className="text-muted-foreground">{c.getValue<string>()}</span> },
      { id: "service", header: "Service", size: 150, enableSorting: false, accessorFn: (c) => serviceTitle(c.service, titles) ?? "", cell: (c) => c.getValue<string>() || dim },
      {
        id: "operation",
        header: "Operation",
        size: 210,
        enableSorting: false,
        accessorFn: (c) => c.action ?? "",
        cell: (c) => (
          // The row's own control: a click anywhere on the row opens the drawer, and so does this from the keyboard.
          <button type="button" className="max-w-full truncate rounded-sm text-left font-semibold text-bright outline-none hover:text-primary focus-visible:ring-2 focus-visible:ring-ring/50" aria-label={`Open call ${c.row.original.seq}: ${c.row.original.action ?? "unknown operation"}`} data-testid="open-call">
            {c.getValue<string>() || <span className="font-normal text-muted-foreground">Unknown</span>}
          </button>
        ),
      },
      { id: "resource", header: "Resource", enableSorting: false, accessorFn: (c) => c.resource ?? "", meta: { mono: true }, cell: (c) => c.getValue<string>() || dim },
      { id: "caller", header: "Caller", size: 172, enableSorting: false, accessorFn: (c) => c.caller ?? "", meta: { mono: true }, cell: (c) => c.getValue<string>() || dim },
      { id: "result", header: "Result", size: 250, enableSorting: false, accessorFn: (c) => `${c.outcome} ${c.status}${c.error_code ? ` ${c.error_code}` : ""}`, cell: (c) => <Result call={c.row.original} /> },
      { id: "latency", header: "Latency", size: 84, enableSorting: false, accessorFn: (c) => micros(c.duration_us), meta: { numeric: true, mono: true } },
    ],
    [titles],
  );

  const status = connWords(conn, tape.paused);
  const ring = tape.ring;
  return (
    <>
      <PageHeader
        title="Live"
        sub="Every AWS API call this doze-aws answers, as it happens. The record is kept in memory and holds only the newest calls."
        actions={
          <>
            <HeldPill count={tape.paused ? tape.held.length : 0} onShow={resume} />
            <StatusDot tone={status.tone} className="min-w-[6.75rem] justify-end" data-testid="live-status" aria-live="polite">
              {status.word}
            </StatusDot>
            {tape.paused ? (
              <Button variant="outline" className="min-w-[6.5rem] bg-card" onClick={resume} data-testid="live-resume">
                <PlayIcon /> Resume
              </Button>
            ) : (
              <Button variant="outline" className="min-w-[6.5rem] bg-card" onClick={pause} disabled={!tape.loaded} data-testid="live-pause">
                <PauseIcon /> Pause
              </Button>
            )}
          </>
        }
      />

      {!tape.loaded && conn.state === "waiting" && <ErrorBlock what="The calls could not be loaded" error={conn.error} hint="doze-aws is asked again every few seconds. If it stopped, start it; the list fills by itself." onRetry={retry} testId="live-error" />}

      {!tape.loaded && conn.state !== "waiting" && (
        <div className="rounded-xl border bg-card" aria-busy aria-label="Loading the calls" data-testid="live-loading">
          {[64, 48, 72, 56, 60].map((w, i) => (
            <div key={i} className="flex h-(--row-h) items-center gap-6 border-b px-3 last:border-b-0">
              <Skeleton className="h-3.5 w-20" />
              <Skeleton className="h-3.5" style={{ width: `${w * 3}px` }} />
              <Skeleton className="ml-auto h-3.5 w-16" />
            </div>
          ))}
        </div>
      )}

      {tape.loaded && (
        <>
          {conn.state === "waiting" && (
            <Alert
              tone="warn"
              title="The live stream stopped"
              className="mb-5"
              testId="live-stopped"
              action={
                <Button variant="outline" size="sm" className="bg-card" onClick={retry} data-testid="live-retry">
                  Try now
                </Button>
              }
            >
              {errorWords(conn.error).message} Calls made since are not shown. The stream is opened again by itself every few seconds.
            </Alert>
          )}
          {tape.missed > 0 && (
            <Alert tone="info" title={`${number(tape.missed)} ${tape.missed === 1 ? "call is" : "calls are"} not shown`} className="mb-5" testId="live-missed">
              They were recorded and dropped before this page was given them: the record keeps only the newest {ring ? number(ring.capacity) : ""} calls.
            </Alert>
          )}
          {ring && (
            <Figures
              className="mb-5"
              aria-label="The record"
              figures={[
                { label: "Calls recorded", value: number(ring.recorded), ch: 6, tip: "Since doze-aws started.", testId: "live-recorded" },
                { label: "Held in the record", value: `${number(ring.held)} of ${number(ring.capacity)}`, ch: 13, tip: "The record keeps the newest calls, in memory.", testId: "live-held" },
                { label: "Memory", value: bytes(ring.bytes), ch: 8, tip: "What the held calls take.", testId: "live-bytes" },
              ]}
            />
          )}
          <DataTable
            aria-label="Calls, newest first"
            data={tape.calls}
            columns={columns}
            rowId={(c) => String(c.seq)}
            maxHeight={640}
            anchorTop
            onRowClick={(c) => open(c.seq)}
            rowClassName={(c) => (c.seq === openSeq ? "[&>td]:bg-accent" : c.seq > tape.freshAfter ? "row-new" : undefined)}
            empty={
              <EmptyState icon={RadioIcon} title="No calls yet" className="w-full border-0 bg-transparent">
                {about ? (
                  <>
                    Point an AWS SDK or the AWS CLI at <code className="text-bright">{about.endpoint}</code>. Each call appears here as it is answered, without a reload.
                  </>
                ) : (
                  "Each call to doze-aws appears here as it is answered, without a reload."
                )}
              </EmptyState>
            }
          />
        </>
      )}

      <CallSheet seq={tape.loaded ? openSeq : undefined} call={findCall(tape, openSeq)} serviceTitle={serviceTitle(findCall(tape, openSeq)?.service ?? null, titles)} onClose={() => open(undefined)} />
    </>
  );
}
