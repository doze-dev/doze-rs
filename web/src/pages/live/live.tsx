// The Live page (boards live-wire, live-quiet, live-load, live-lab-rule; classic pages/live/page.rs). A snapshot of the newest
// requests by default, with the RefreshControl; a Live tail when the person switches it on, at the speed they chose. Calm by
// rule (live-design.md, CON-1250 to CON-1262): the header, the strip and the toolbar are never part of an update; the tape's
// columns are fixed; a batch of new rows comes no faster than the pace, is prepended with a brief highlight on the new rows
// only, and is held (a pill counts it) while the person scrolls, hovers, selects, types in the table or has an overlay open.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowUpIcon, ChevronLeftIcon, ChevronRightIcon, ChevronDownIcon, FilterIcon, NetworkIcon, RadioIcon, SearchIcon, XIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useLive } from "@/api/live";
import type { LiveResponse } from "@/api/types/LiveResponse";
import type { WireRow } from "@/api/types/WireRow";
import { MultiSelect } from "@/components/app/multi-select";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { hhmmUtc, number } from "@/lib/format";
import { useTailOwnsRefresh } from "@/lib/refresh";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";
import { ClientsSheet } from "./clients-sheet";
import { RequestSheet } from "./request-sheet";
import { type LiveSearch, apiQuery, filtered } from "./search";
import { type Conn, SPEEDS, type Speed, TAPE_MOST, type TapeRow, TailStream, hold, merge, newCount, parseSpeed, speedSecs, useSpeed } from "./tail";
import { type FilterBy, Tape } from "./tape";
import { clockMs } from "./words";

/** The tape's height: a box of its own, so a batch never moves the page. */
const TAPE_HEIGHT = 600;

function stats(r: LiveResponse): Stat[] {
  const v = r.live;
  return [
    { label: "Requests, last minute", value: number(v.last_minute), ch: 7 },
    { label: "Rows kept", value: number(v.window.rows), unit: `of ${number(v.window.capacity)}`, ch: 6 },
    { label: "Reaches back to", value: v.window.oldest_ms ? hhmmUtc(v.window.oldest_ms) : "none", unit: "UTC", mono: true, ch: 5, tip: "The oldest row kept; the wire starts empty at every start" },
    // One definition with the Clients sheet: a client is a client.id with a connection open; its connections are the unit.
    { label: "Clients", value: number(v.clients), unit: v.connections === 1 ? "1 connection" : `${number(v.connections)} connections`, ch: 5 },
  ];
}

/** The refusals, folded to one line per (code, client), in a strip of one line that opens in place. */
function Refusals({ r, onOpen }: { r: LiveResponse; onOpen: (id: number) => void }) {
  const [open, setOpen] = useState(false);
  const v = r.live;
  const total = v.refused_total + v.denied_total;
  return (
    <div className="rounded-xl border bg-card text-[13.5px]" data-testid="refusals">
      <button type="button" disabled={total === 0} aria-expanded={total > 0 ? open : undefined} onClick={() => setOpen((o) => !o)} className="flex h-11 w-full items-center gap-2 rounded-xl px-4 text-left focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none enabled:hover:bg-accent/40">
        {total === 0 ? (
          <span className="text-muted-foreground">No refusals among the rows kept.</span>
        ) : (
          <>
            <ChevronDownIcon className={cn("size-4 shrink-0 text-muted-foreground transition-transform", !open && "-rotate-90")} aria-hidden />
            <b className="font-semibold text-bright">Refusals</b>
            <span className="text-muted-foreground">among the rows kept</span>
            <span className="inline-block min-w-[4ch] font-mono tabular-nums" data-testid="refusals-total">
              {number(total)}
            </span>
            {/* One chip per (code, client): entries never run together. The strip is one line; what does not fit is cut at
                a chip's edge and is in the list that opens. */}
            <span className="ml-1 flex h-6 min-w-0 flex-1 flex-wrap items-center gap-1.5 overflow-hidden" data-testid="refusal-chips">
              {v.refused.map((x) => (
                <span key={`${x.code}-${x.client}`} className="inline-flex h-6 max-w-[18rem] shrink-0 items-center gap-1.5 rounded-full border border-refused/25 bg-card px-2.5 text-[12px] whitespace-nowrap" data-testid="refusal-chip">
                  <span className="font-mono text-refused">{x.code_name}</span>
                  <span className="truncate text-muted-foreground">{x.client || "(no client.id)"}</span>
                </span>
              ))}
            </span>
          </>
        )}
      </button>
      {open && total > 0 && (
        <ul className="flex flex-col border-t px-4 py-2" data-testid="refusal-lines">
          {v.refused.map((x) => (
            <li key={`${x.code}-${x.client}`} className="flex min-w-0 items-center gap-2 py-1.5">
              <span className="font-mono text-[12.5px] text-refused">
                {x.code_name} ({x.code})
              </span>
              <span className="text-muted-foreground">·</span>
              <span className="min-w-0 truncate font-mono text-[12.5px]">{x.client}</span>
              <span className="text-muted-foreground">·</span>
              <span className="font-mono text-[12.5px] tabular-nums">×{number(x.count)}</span>
              <span className="text-muted-foreground">· last</span>
              <span className="font-mono text-[12.5px]">{clockMs(x.at_ms).slice(0, 8)}</span>
              <Button variant="link" size="sm" className="h-auto px-1" onClick={() => onOpen(x.id)}>
                View
              </Button>
            </li>
          ))}
          {v.refused_more > 0 && <li className="py-1.5 text-muted-foreground">and {v.refused_more} more kinds</li>}
        </ul>
      )}
    </div>
  );
}

function SearchBox({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [text, setText] = useState(value);
  const [prev, setPrev] = useState(value);
  if (value !== prev) {
    setPrev(value);
    setText(value);
  }
  useEffect(() => {
    if (text === value) return;
    const t = setTimeout(() => onChange(text), 350);
    return () => clearTimeout(t);
  }, [text, value, onChange]);
  return (
    <div className="relative w-full max-w-[18rem] min-w-[14rem] flex-1">
      <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
      <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Filter by client, API or topic" aria-label="Filter the wire" className="bg-card pl-9" maxLength={128} data-testid="live-search" />
    </div>
  );
}

/** The connection, in a slot of fixed width: grey Snapshot, green Live, amber Reconnecting, red Disconnected with its countdown. */
function Indicator({ on, conn, onRetry }: { on: boolean; conn: Conn; onRetry: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  const counting = on && conn.state === "disconnected" && conn.retryAt !== undefined;
  useEffect(() => {
    if (!counting) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [counting]);
  const [tone, word] = !on
    ? ["bg-muted-foreground", "Snapshot"]
    : conn.state === "live"
      ? ["bg-good", "Live"]
      : conn.state === "connecting"
        ? ["bg-warn", "Connecting…"]
        : conn.state === "reconnecting"
          ? ["bg-warn", "Reconnecting…"]
          : ["bg-bad", conn.retryAt ? `Disconnected · Retry in ${Math.max(1, Math.ceil((conn.retryAt - now) / 1000))} s` : "Disconnected · Retrying…"];
  return (
    <span className="inline-flex min-w-[14rem] items-center gap-2 text-[13px]" data-testid="tail-state" data-state={on ? conn.state : "snapshot"} role="status">
      <span aria-hidden className={cn("size-2 shrink-0 rounded-full", tone, on && conn.state === "live" && "shadow-[0_0_0_3px] shadow-good/20")} />
      <span className={cn("font-medium whitespace-nowrap tabular-nums", !on ? "text-muted-foreground" : "text-bright")}>{word}</span>
      {on && conn.state === "disconnected" && (
        <Button variant="link" size="sm" className="h-auto px-1" onClick={onRetry} data-testid="tail-retry">
          Retry now
        </Button>
      )}
    </span>
  );
}

type TailView = { rows: TapeRow[] | null; heldCount: number; conn: Conn };

/** What the tail shows, outside React: the stream writes it, the page reads it (one render per change). */
class TailStore {
  view: TailView = { rows: null, heldCount: 0, conn: { state: "connecting" } };
  private listeners = new Set<() => void>();
  subscribe = (l: () => void) => {
    this.listeners.add(l);
    return () => void this.listeners.delete(l);
  };
  get = () => this.view;
  set(patch: Partial<TailView>) {
    this.view = { ...this.view, ...patch };
    for (const l of this.listeners) l();
  }
}

/** The tape and its tail: rows merged by id, at most one batch a pace, held while the person reads. */
function useTailTape(on: boolean, query: string, snap: LiveResponse | undefined, ready: boolean, pace: number, held: () => boolean) {
  const [store] = useState(() => new TailStore());
  const { rows, heldCount, conn } = useSyncExternalStore(store.subscribe, store.get, store.get);
  const ctl = useRef({ show: () => {}, retry: () => {} });
  const snapRef = useRef(snap);
  // Where a tail that restarts (a new speed) carries on from, so that the tape is not set back to the snapshot.
  const kept = useRef<{ query: string; tape: TapeRow[]; cursor: string } | null>(null);
  useEffect(() => {
    snapRef.current = snap;
  }, [snap]);
  useEffect(() => {
    const setRows = (r: TapeRow[] | null) => store.set({ rows: r });
    const setHeldCount = (n: number) => store.set({ heldCount: n });
    const setConn = (c: Conn) => store.set({ conn: c });
    if (!on) {
      kept.current = null;
      setRows(null);
      setHeldCount(0);
      return;
    }
    const s = snapRef.current;
    if (!ready || !s) return;
    const carry = kept.current?.query === query ? kept.current : null;
    let tape: TapeRow[] = carry?.tape ?? s.live.rows.slice(0, TAPE_MOST);
    let aside: WireRow[] = [];
    let last = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const keep = (cursor?: string) => {
      kept.current = { query, tape, cursor: cursor ?? kept.current?.cursor ?? s.tail_from };
    };
    keep(carry?.cursor ?? s.tail_from);
    setRows(tape);
    setHeldCount(0);
    const apply = () => {
      tape = merge(tape, aside).rows;
      aside = [];
      last = Date.now();
      keep();
      setRows(tape);
      setHeldCount(0);
    };
    const tick = () => {
      timer = undefined;
      if (aside.length === 0) return;
      if (held()) {
        setHeldCount(newCount(aside, tape));
        timer = setTimeout(tick, pace * 1000);
        return;
      }
      apply();
    };
    const stream = new TailStream(query, kept.current!.cursor, pace, {
      onReady: (r) => {
        if (r.reset) {
          // The broker started again: its tape is a new one, and an id of the old life could be another request.
          tape = [];
          aside = [];
          setRows(tape);
          setHeldCount(0);
        }
        keep(r.cursor);
      },
      onBatch: (b) => {
        aside = hold(aside, b.rows);
        keep(b.cursor);
        if (!timer) timer = setTimeout(tick, Math.max(0, last + pace * 1000 - Date.now()));
      },
      onConn: setConn,
    });
    ctl.current = {
      show: () => {
        clearTimeout(timer);
        timer = undefined;
        if (aside.length) apply();
      },
      retry: () => stream.retry(),
    };
    stream.start();
    return () => {
      stream.stop();
      clearTimeout(timer);
    };
  }, [on, query, ready, pace, held, store]);
  return { rows, heldCount, conn, show: () => ctl.current.show(), retry: () => ctl.current.retry() };
}

export function LivePage() {
  const search = useSearch({ from: "/live" });
  const navigate = useNavigate({ from: "/live" });
  const on = !!search.tail;
  const query = apiQuery(search, !on);
  const q = useLive(query, !on);
  const r = q.data;
  const ready = !!r && !q.isPlaceholderData;
  const [speed, setSpeed] = useSpeed();
  useTailOwnsRefresh();
  const pace = speedSecs(speed);

  const navRef = useRef(navigate);
  useEffect(() => {
    navRef.current = navigate;
  }, [navigate]);
  const set = useCallback((patch: Partial<LiveSearch>, replace = true) => void navRef.current({ search: (s: LiveSearch) => ({ ...s, ...patch }), replace, resetScroll: false }), []);
  const filterBy: FilterBy = useCallback((field, value) => set({ [field]: [value], before: undefined }, false), [set]);
  const onSearch = useCallback((text: string) => set({ q: text.trim() || undefined, before: undefined }), [set]);

  // An overlay opened from this page pushed an entry, so closing goes Back; opened by a deep link, closing replaces.
  const pushed = useRef(false);
  const openOverlay = useCallback(
    (patch: Partial<LiveSearch>) => {
      pushed.current = true;
      set(patch, false);
    },
    [set],
  );
  const closeOverlay = (patch: Partial<LiveSearch>) => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else set(patch);
  };

  // What holds the tape: scrolled into it, the pointer on it, a selection or the focus in it, an overlay or a popup open.
  const wrap = useRef<HTMLDivElement>(null);
  const hovering = useRef(false);
  const overlayOpen = useRef(false);
  useEffect(() => {
    overlayOpen.current = search.request !== undefined || !!search.clients;
  }, [search.request, search.clients]);
  const held = useCallback(() => {
    const el = wrap.current;
    if (!el) return false;
    const scroller = el.querySelector<HTMLElement>("[data-slot=data-table]");
    if ((scroller?.scrollTop ?? 0) > 2) return true;
    if (el.getBoundingClientRect().top < 0) return true;
    if (hovering.current || overlayOpen.current) return true;
    if (document.querySelector("[role=dialog], [data-radix-popper-content-wrapper]")) return true;
    if (el.contains(document.activeElement)) return true;
    const sel = document.getSelection();
    return !!sel && !sel.isCollapsed && el.contains(sel.anchorNode);
  }, []);
  const tail = useTailTape(on, query, r, ready, pace, held);

  const switchMode = (v: string) => {
    if (v === "tail" && !on) set({ tail: true, before: undefined });
    if (v === "snapshot" && on) {
      set({ tail: undefined });
      // Back to a snapshot: one fetch, so the tape is the newest and not where the tail left it.
      void q.refetch();
    }
  };
  const showHeld = () => {
    wrap.current?.querySelector<HTMLElement>("[data-slot=data-table]")?.scrollTo({ top: 0 });
    tail.show();
  };

  const rows: TapeRow[] = on && tail.rows ? tail.rows : (r?.live.rows ?? []);
  const facets = r?.live.facets;
  const opts = (f: { value: string; count: number }[] | undefined, chosen: string[] | undefined) => {
    const out = (f ?? []).map((o) => ({ value: o.value, label: o.value, hint: number(o.count) }));
    for (const c of chosen ?? []) if (!out.some((o) => o.value === c)) out.push({ value: c, label: c, hint: "0" });
    return out;
  };
  const empty = r ? (
    filtered(search) ? (
      <div className="flex flex-col items-center gap-3 text-center" data-testid="live-no-match">
        <FilterIcon className="size-5 text-link" aria-hidden />
        <b className="text-[15px] font-semibold text-bright">No request matches these filters</b>
        <p className="max-w-lg">
          Among the {number(r.live.window.rows)} rows kept, none has all of: {r.words.phrases.join("; ")}.
        </p>
        <Button variant="outline" size="sm" className="bg-card" onClick={() => set({ q: undefined, client: undefined, api: undefined, topic: undefined, refused: undefined, before: undefined }, false)}>
          <XIcon /> Clear filters
        </Button>
      </div>
    ) : (
      <span className="max-w-xl text-center font-mono text-[12.5px]" data-testid="live-empty">
        {r.words.empty}
      </span>
    )
  ) : undefined;

  return (
    <>
      <PageHeader
        title="Live"
        sub="Everything your apps send, newest first. Repeats fold into one row with a count; the oldest rows fall off."
        actions={
          <Button variant="outline" className="bg-card" disabled={!r} onClick={() => openOverlay({ clients: true })} data-testid="open-clients">
            <NetworkIcon /> Clients <span className="inline-block min-w-[3ch] text-left font-mono text-muted-foreground tabular-nums">{r ? number(r.live.clients) : ""}</span>
          </Button>
        }
      />
      {q.isError && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !r ? (
        <div className="flex flex-col gap-4" aria-busy>
          <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
            {Array.from({ length: 4 }, (_, i) => (
              <Skeleton key={i} className="h-[98px] rounded-xl" />
            ))}
          </div>
          <Skeleton className="h-11 rounded-xl" />
          <Skeleton className="h-[600px] rounded-xl" />
        </div>
      ) : (
        <div className="flex flex-col gap-4" data-testid="live">
          <StatStrip stats={stats(r)} />
          <Refusals r={r} onOpen={(id) => openOverlay({ request: id })} />
          <div className="flex flex-wrap items-center gap-2" data-testid="live-toolbar">
            <SearchBox value={search.q ?? ""} onChange={onSearch} />
            <MultiSelect options={opts(facets?.clients, search.client)} value={search.client ?? []} onChange={(v) => set({ client: v.length ? v : undefined, before: undefined }, false)} noun="apps" aria-label="Apps" triggerClassName="w-36" chips={false} />
            <MultiSelect options={opts(facets?.topics, search.topic)} value={search.topic ?? []} onChange={(v) => set({ topic: v.length ? v : undefined, before: undefined }, false)} noun="topics" aria-label="Topics" triggerClassName="w-36" chips={false} />
            <MultiSelect options={opts(facets?.apis, search.api)} value={search.api ?? []} onChange={(v) => set({ api: v.length ? v : undefined, before: undefined }, false)} noun="APIs" aria-label="APIs" triggerClassName="w-36" chips={false} />
            <Toggle variant="outline" pressed={!!search.refused} onPressedChange={(p) => set({ refused: p ? true : undefined, before: undefined }, false)} className="bg-card px-3 data-[state=on]:border-primary/40 data-[state=on]:bg-primary/10 data-[state=on]:text-link" data-testid="refused-only">
              <FilterIcon /> Refused only
            </Toggle>
            <div className="flex h-9 items-center gap-2 px-1">
              <Switch id="live-idle" checked={!search.idle} onCheckedChange={(c) => set({ idle: c ? undefined : true, before: undefined }, false)} data-testid="idle-hidden" />
              <Label htmlFor="live-idle" className="text-[13.5px] font-normal whitespace-nowrap">
                Idle polls hidden
              </Label>
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2" data-testid="tail-bar">
            <ToggleGroup type="single" variant="outline" value={on ? "tail" : "snapshot"} onValueChange={(v) => v && switchMode(v)} aria-label="Mode" className="bg-card" data-testid="tail-mode">
              <ToggleGroupItem value="snapshot" className="px-3">
                Snapshot
              </ToggleGroupItem>
              <ToggleGroupItem value="tail" className="px-3">
                <RadioIcon /> Live tail
              </ToggleGroupItem>
            </ToggleGroup>
            <Select value={speed} onValueChange={(v) => setSpeed(parseSpeed(v) as Speed)}>
              <SelectTrigger size="sm" className="w-[9.5rem] bg-card" aria-label="Tail speed" data-testid="tail-speed">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SPEEDS.map((s) => (
                  <SelectItem key={s.value} value={s.value}>
                    {s.label} · {s.secs} s
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Indicator on={on} conn={tail.conn} onRetry={tail.retry} />
            <span className="flex-1" />
            {/* The switch owns how the tape keeps up (no page-wide interval here): a snapshot is refreshed by hand, a tail by
                itself. */}
            {!on && <RefreshControl interval={false} updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} />}
          </div>
          <div className="relative" ref={wrap} onPointerEnter={() => (hovering.current = true)} onPointerLeave={() => (hovering.current = false)} data-testid="tape-wrap" data-tail={on || undefined}>
            {/* The pill floats over the top of the tape: it takes no room, so it moves nothing when it comes and goes. */}
            {on && tail.heldCount > 0 && (
              <Button size="sm" onClick={showHeld} className="absolute top-12 left-1/2 z-20 -translate-x-1/2 rounded-full shadow-md" data-testid="held-pill">
                <ArrowUpIcon /> <span className="tabular-nums">{number(tail.heldCount)}</span> new · Show
              </Button>
            )}
            <Tape rows={rows} layout="live" filter={filterBy} empty={empty} maxHeight={TAPE_HEIGHT} label="The wire" onOpen={(row) => openOverlay({ request: row.id })} testId="tape" />
          </div>
          <div className="flex flex-wrap items-center justify-end gap-3 text-[13px] text-muted-foreground" data-testid="pager">
            <span className="whitespace-nowrap tabular-nums">
              {on ? (
                <>
                  Showing the newest <span className="inline-block min-w-[3ch] text-right">{number(rows.length)}</span> rows, at most {number(TAPE_MOST)}
                </>
              ) : (
                <>
                  Showing <span className="inline-block min-w-[3ch] text-right">{number(rows.length)}</span> of <span className="inline-block min-w-[5ch] text-right">{number(r.live.matched)}</span> matching rows
                </>
              )}
              {!search.idle && " · idle polls hidden"}
            </span>
            <Button variant="outline" size="sm" className="bg-card" disabled={on || search.before === undefined} onClick={() => set({ before: undefined }, false)}>
              <ChevronLeftIcon /> Newer
            </Button>
            <Button variant="outline" size="sm" className="bg-card" disabled={on || r.live.older === null} onClick={() => r.live.older !== null && set({ before: r.live.older }, false)}>
              Older <ChevronRightIcon />
            </Button>
          </div>
        </div>
      )}
      <RequestSheet id={search.request} onClose={() => closeOverlay({ request: undefined })} />
      <ClientsSheet open={!!search.clients} onClose={() => closeOverlay({ clients: undefined })} />
    </>
  );
}
