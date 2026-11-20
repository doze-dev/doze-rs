import { getRouteApi } from "@tanstack/react-router";
import { ChevronDownIcon, ChevronRightIcon, PauseIcon, PlayIcon, RadioIcon, SearchXIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { HeldPill } from "@doze/ui/components/app/held-pill";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Figures } from "@doze/ui/components/app/stat";
import { StatusDot, type Tone } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { bytes, clockMsUtc, micros, number } from "@doze/ui/lib/format";
import { useTailOwnsRefresh } from "@doze/ui/lib/refresh";
import { useAbout } from "../../api/queries";
import type { CallRecord } from "../../api/types/CallRecord";
import { serviceTitle } from "../../words";
import { RowMenu } from "./actions";
import { CallSheet } from "./call-sheet";
import { madeBy } from "./cause";
import { Filters } from "./filters";
import { flowKeyOf, layout, withoutConsole } from "./flows";
import { LiveMap, MapLegend } from "./map";
import { Result } from "./outcome";
import { type Line, lineId, lines } from "./repeats";
import { resourceShown } from "./resource";
import { callerOf, consoleShown, filtering, isConsole, type LiveSearch, matches, type View } from "./search";
import { findCall } from "./tape";
import { useFlows } from "./use-flows";
import { type Conn, useTail } from "./use-tail";

const route = getRouteApi("/live");

const dim = <span className="text-muted-foreground">–</span>;

function connWords(conn: Conn, paused: boolean): { tone: Tone; word: string } {
  if (conn.state === "waiting") return { tone: "warn", word: "Reconnecting" };
  if (conn.state === "connecting") return { tone: "info", word: "Connecting" };
  return paused ? { tone: "idle", word: "Paused" } : { tone: "ok", word: "Live" };
}

function Caller({ call, titles }: { call: CallRecord; titles: ReadonlyMap<string, string> }) {
  if (!call.via) return call.caller ? <span className="truncate">{call.caller}</span> : dim;
  const by = call.via.by ? (serviceTitle(call.via.by, titles) ?? call.via.by) : (call.caller ?? "");
  return (
    <span className="flex min-w-0 items-center gap-1.5" title={`Made by ${madeBy(call, titles)}, for ${call.caller ?? "no key"}`}>
      <span className="truncate font-sans text-[13px]">{by}</span>
      <span className="shrink-0 rounded-full border bg-muted px-1.5 font-sans text-[10.5px] leading-4 text-muted-foreground" data-testid="via-pill">
        {call.via.name ? `via ${call.via.name.split(" ")[0]}` : "via"}
      </span>
    </span>
  );
}

/** The line that says how many of this console's own calls are hidden, with the switch that shows them (kept in the address). */
function ConsoleLine({ hidden, shown, set }: { hidden: number; shown: boolean; set: (patch: Partial<LiveSearch>) => void }) {
  if (!shown && hidden === 0) return null;
  return (
    <p className="mb-4 flex flex-wrap items-center gap-x-2 text-[13px] text-muted-foreground" data-testid="console-hidden">
      {shown ? (
        <span>This console's own calls are shown, marked with the caller console.</span>
      ) : (
        <span className="tabular-nums">
          {number(hidden)} {hidden === 1 ? "call" : "calls"} made by this console {hidden === 1 ? "is" : "are"} hidden
        </span>
      )}
      <span aria-hidden>·</span>
      <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => set({ console: shown ? undefined : "shown" })} data-testid="console-toggle">
        {shown ? "Hide them" : "Show"}
      </button>
    </p>
  );
}

/** Live: every AWS API call this doze-aws answers, and every call one service makes to another, as it happens. The map draws
 * who calls what; the wire lists the calls, newest first, repeats folded. Every value on screen is a field of the contract. */
export function LivePage() {
  useTailOwnsRefresh();
  const { tape, conn, pause, resume, retry } = useTail();
  const about = useAbout().data;
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const set = useCallback((patch: Partial<LiveSearch>) => void navigate({ search: (prev) => ({ ...prev, ...patch }), replace: true }), [navigate]);
  const open = useCallback((seq: number | undefined) => void navigate({ search: (prev) => ({ ...prev, call: seq }) }), [navigate]);
  const view: View = search.view ?? "both";
  const titles = useMemo(() => new Map((about?.services ?? []).map((s) => [s.name, s.title])), [about]);
  const { flows: allFlows } = useFlows(tape.calls, tape.cursor);
  // This console's own calls (its Resources pages' reads and writes) are noise while watching an app: hidden unless asked for.
  const mine = consoleShown(search);
  const flows = useMemo(() => (mine ? allFlows : withoutConsole(allFlows)), [mine, allFlows]);
  const map = useMemo(() => layout(flows), [flows]);
  const visible = useMemo(() => (mine ? tape.calls : tape.calls.filter((c) => !isConsole(c))), [mine, tape.calls]);
  const hidden = tape.calls.length - visible.length;
  const [runs, setRuns] = useState<ReadonlySet<number>>(new Set());
  const toggle = useCallback((id: number) => setRuns((was) => (was.has(id) ? new Set([...was].filter((x) => x !== id)) : new Set([...was, id]))), []);

  const kept = useMemo(() => (filtering(search) ? visible.filter((c) => matches(c, search)) : visible), [visible, search]);
  const shown = useMemo(() => lines(kept, runs), [kept, runs]);
  const services = useMemo(() => [...new Set([...flows.byKey.values()].map((f) => f.service).filter(Boolean))].sort(), [flows]);
  const callers = useMemo(() => {
    const seen = new Map<string, boolean>();
    for (const f of flows.byKey.values()) seen.set(f.caller, f.internal);
    return [...seen].map(([id, internal]) => ({ id, internal })).sort((a, b) => Number(a.internal) - Number(b.internal) || a.id.localeCompare(b.id));
  }, [flows]);
  const openCall = findCall(tape, search.call);

  const columns = useMemo<DataColumn<Line>[]>(
    () => [
      // The widths add up to what a 1440 px window leaves beside the rail, with the rest for Resource. The time is never cut; a
      // long service title, operation or error code is, with the whole value on hover and in the drawer.
      {
        id: "time",
        header: "Time",
        size: 122,
        enableSorting: false,
        accessorFn: (l) => clockMsUtc(l.call.at_ms),
        meta: { mono: true },
        cell: (c) => <span className={c.row.original.kind === "member" ? "pl-3 text-muted-foreground/80" : "text-muted-foreground"}>{c.getValue<string>()}</span>,
      },
      { id: "service", header: "Service", size: 124, enableSorting: false, accessorFn: (l) => serviceTitle(l.call.service, titles) ?? "", cell: (c) => c.getValue<string>() || dim },
      {
        id: "operation",
        header: "Operation",
        size: 214,
        enableSorting: false,
        accessorFn: (l) => l.call.action ?? "",
        cell: (c) => {
          const l = c.row.original;
          return (
            <span className="flex min-w-0 items-center gap-2">
              {/* The row's own control: a click anywhere on the row opens the drawer, and so does this from the keyboard. */}
              <button type="button" className="max-w-full truncate rounded-sm text-left font-semibold text-bright outline-none hover:text-primary focus-visible:ring-2 focus-visible:ring-ring/50" aria-label={`Open call ${l.call.seq}: ${l.call.action ?? "unknown operation"}`} data-testid="open-call">
                {c.getValue<string>() || <span className="font-normal text-muted-foreground">Unknown</span>}
              </button>
              {l.kind === "run" && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    toggle(l.calls[l.calls.length - 1]!.seq);
                  }}
                  className="inline-flex shrink-0 items-center gap-0.5 rounded-full border bg-muted px-1.5 font-mono text-[11px] leading-[18px] text-foreground outline-none hover:border-ring focus-visible:ring-2 focus-visible:ring-ring"
                  aria-expanded={l.open}
                  aria-label={`${l.calls.length} calls in a row like this one: ${l.open ? "fold them" : "show each"}`}
                  data-testid="repeat-count"
                >
                  {l.open ? <ChevronDownIcon className="size-3" /> : <ChevronRightIcon className="size-3" />}×{number(l.calls.length)}
                </button>
              )}
            </span>
          );
        },
      },
      { id: "resource", header: "Resource", enableSorting: false, accessorFn: (l) => resourceShown(l.call), meta: { mono: true }, cell: (c) => (c.getValue<string>() ? <span title={c.row.original.call.resource ?? undefined}>{c.getValue<string>()}</span> : dim) },
      { id: "caller", header: "Caller", size: 186, enableSorting: false, accessorFn: (l) => callerOf(l.call), meta: { mono: true }, cell: (c) => <Caller call={c.row.original.call} titles={titles} /> },
      { id: "result", header: "Result", size: 214, enableSorting: false, accessorFn: (l) => `${l.call.outcome} ${l.call.status}${l.call.error_code ? ` ${l.call.error_code}` : ""}`, cell: (c) => <Result call={c.row.original.call} /> },
      { id: "latency", header: "Latency", size: 84, enableSorting: false, accessorFn: (l) => micros(l.call.duration_us), meta: { numeric: true, mono: true } },
      { id: "actions", header: () => <span className="sr-only">Actions</span>, size: 44, enableSorting: false, cell: (c) => <RowMenu call={c.row.original.call} /> },
    ],
    [titles, toggle],
  );

  const status = connWords(conn, tape.paused);
  const ring = tape.ring;
  return (
    <>
      <PageHeader
        title="Live"
        sub="Every AWS API call this doze-aws answers, and every call one service makes to another, as it happens. Kept in memory: only the newest calls."
        actions={
          <>
            <HeldPill count={tape.paused ? tape.held.length : 0} onShow={resume} />
            <StatusDot tone={status.tone} className="min-w-[6.75rem] justify-end" data-testid="live-status" aria-live="polite">
              {status.word}
            </StatusDot>
            <ToggleGroup type="single" variant="outline" size="sm" value={view} onValueChange={(v) => v && set({ view: v === "both" ? undefined : (v as View) })} aria-label="View" className="bg-card" data-testid="live-view">
              <ToggleGroupItem value="map" className="px-3" data-testid="view-map">
                Map
              </ToggleGroupItem>
              <ToggleGroupItem value="wire" className="px-3" data-testid="view-wire">
                Wire
              </ToggleGroupItem>
              <ToggleGroupItem value="both" className="px-3" data-testid="view-both">
                Both
              </ToggleGroupItem>
            </ToggleGroup>
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
          <ConsoleLine hidden={hidden} shown={search.console === "shown"} set={set} />
          {view !== "wire" && (
            <section className="mb-5 rounded-xl border bg-card px-5 pt-3 pb-4" aria-label="Map of who calls what" data-testid="live-map-card">
              {map.lines.length === 0 ? (
                <EmptyState icon={RadioIcon} title="Nothing to draw yet" className="border-0 bg-transparent">
                  Each caller and each service it calls is drawn here, joined by a line, as calls arrive.
                </EmptyState>
              ) : (
                <>
                  <LiveMap layout={map} calls={visible} titles={titles} selected={openCall ? flowKeyOf(openCall) : null} caller={search.caller} service={search.service} onCaller={(caller) => set({ caller })} onService={(service) => set({ service })} />
                  <MapLegend />
                </>
              )}
            </section>
          )}
          {view !== "map" && (
            <>
              <Filters search={search} set={set} services={services} callers={callers} titles={titles} shown={kept.length} held={visible.length} />
              <DataTable
                aria-label="Calls, newest first"
                data={shown}
                columns={columns}
                rowId={lineId}
                maxHeight={view === "both" ? 520 : 640}
                anchorTop
                onRowClick={(l) => open(l.call.seq)}
                rowClassName={(l) => (l.call.seq === search.call ? "[&>td]:bg-accent" : l.kind === "member" ? "[&>td]:bg-muted/50" : l.call.seq > tape.freshAfter ? "row-new" : l.call.via ? "[&>td]:text-muted-foreground" : undefined)}
                empty={
                  filtering(search) && visible.length > 0 ? (
                    <EmptyState icon={SearchXIcon} title="No call matches" className="w-full border-0 bg-transparent">
                      None of the {number(visible.length)} calls held matches these filters.
                    </EmptyState>
                  ) : hidden > 0 && visible.length === 0 ? (
                    <EmptyState icon={RadioIcon} title="Only this console's calls so far" className="w-full border-0 bg-transparent">
                      The {number(hidden)} calls held were made by this console's own pages. Calls from your apps, the AWS CLI and between services appear here as they are answered.
                    </EmptyState>
                  ) : (
                    <EmptyState icon={RadioIcon} title="No calls yet" className="w-full border-0 bg-transparent">
                      {about ? (
                        <>
                          Point an AWS SDK or the AWS CLI at <code className="text-bright">{about.endpoint}</code>. Each call appears here as it is answered, without a reload.
                        </>
                      ) : (
                        "Each call to doze-aws appears here as it is answered, without a reload."
                      )}
                    </EmptyState>
                  )
                }
              />
            </>
          )}
        </>
      )}

      <CallSheet seq={tape.loaded ? search.call : undefined} call={openCall} serviceTitle={serviceTitle(openCall?.service ?? null, titles)} titles={titles} held={tape.calls} onOpen={open} onClose={() => open(undefined)} />
    </>
  );
}
