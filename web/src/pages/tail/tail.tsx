// The Live tail page (`/tail`, CON-1580 to CON-1585): "did my service emit that event, and what was in it?", across one topic
// or several. A person chooses topics (by name or a glob pattern) and where to start; the console follows them in one merged
// stream, newest first, filtered on the server, batched at the pace chosen (CON-809), held while the person reads, paused when
// asked or when the tab is hidden, and resumed from its cursor. A record opens in the Messages page's Sheet.
import { getRouteApi } from "@tanstack/react-router";
import { RadioIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { toast } from "sonner";
import { useShell } from "@/api/queries";
import { useTopics } from "@/api/topics";
import type { TailRecord } from "@/api/types/TailRecord";
import type { TopicsTailReady } from "@/api/types/TopicsTailReady";
import type { Option } from "@/components/app/combobox";
import { CopyButton } from "@/components/app/copy-button";
import { DataTable } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { typing } from "@/components/app/refresh-control";
import { type Stat, StatStrip } from "@/components/app/stat";
import { HeldPill } from "@/components/app/tail-controls";
import { Button } from "@/components/ui/button";
import { number } from "@/lib/format";
import { clock } from "@/lib/kafka";
import { useTailOwnsRefresh } from "@/lib/refresh";
import { shellWord } from "@/lib/shell";
import { speedSecs, useSpeed } from "../live/tail";
import { RecordSheet } from "../topics/record";
import { ChipLine, FollowBar, type Patch } from "./controls";
import { type TailSearch, askQuery, following, recordOf } from "./search";
import { type Conn, MOST_HELD, type Row, type Sample, TopicsTail, WINDOW, hold, huesOf, prepend, rates, rowKey } from "./stream";
import { columns } from "./table";

const route = getRouteApi("/tail");
const WIDE = "(min-width: 1280px)";
const TABLE_HEIGHT = 640;

function useWide(): boolean {
  return useSyncExternalStore(
    (l) => {
      const m = window.matchMedia(WIDE);
      m.addEventListener("change", l);
      return () => m.removeEventListener("change", l);
    },
    () => window.matchMedia(WIDE).matches,
    () => true,
  );
}

function useSet() {
  const navigate = route.useNavigate();
  return useCallback((patch: Patch, push = false) => void navigate({ search: (s: TailSearch) => ({ ...s, ...patch }), replace: !push, resetScroll: false }), [navigate]);
}

/** The keys of the page (`typing` keeps them out of fields, menus and dialogs): p pause, n show new, / filters, c clear. */
function useKeys(on: Record<string, () => void>) {
  const ref = useRef(on);
  useEffect(() => {
    ref.current = on;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || typing(e)) return;
      const f = ref.current[e.key];
      if (f) {
        e.preventDefault();
        f();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

type Totals = { matched: number; scanned: number; skipped: number };

function stats(rows: number, t: Totals, perSec: string): Stat[] {
  return [
    { label: "On screen", value: number(rows), ch: 4, unit: `of at most ${WINDOW}` },
    { label: "Matched", value: number(t.matched), ch: 7, unit: "kept by the filters" },
    { label: "Read", value: number(t.scanned), ch: 7, unit: "written since it opened" },
    { label: "Per second", value: perSec, ch: 5, unit: "written, over 10 s" },
    { label: "Skipped", value: number(t.skipped), ch: 6, unit: t.skipped > 0 ? "too far behind to read" : "every record was read" },
  ];
}

function Following({ search, set, options, address, onOpen }: { search: TailSearch; set: (p: Patch, push?: boolean) => void; options: Option[]; address: string; onOpen: (r: Row) => void }) {
  const ask = askQuery(search);
  const [speed] = useSpeed();
  const pace = speedSecs(speed);
  const [rows, setRows] = useState<Row[]>([]);
  const [heldCount, setHeldCount] = useState(0);
  const [conn, setConn] = useState<Conn>({ state: "connecting" });
  const [ready, setReady] = useState<TopicsTailReady | null>(null);
  const [followed, setFollowed] = useState<string[]>([]);
  const [totals, setTotals] = useState<Totals>({ matched: 0, scanned: 0, skipped: 0 });
  const [paused, setPaused] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [samples, setSamples] = useState<Sample[]>([]);
  const heldBuf = useRef<TailRecord[]>([]);
  const tail = useRef<TopicsTail | null>(null);
  const pausedRef = useRef(false);
  const wrap = useRef<HTMLDivElement>(null);
  const hovering = useRef(false);
  const [startedAt] = useState(() => Date.now());

  // What holds the stream: scrolled into it, the pointer on it, a selection or the focus in it, an overlay or a popup open.
  const isHeld = useCallback(() => {
    const el = wrap.current;
    if (!el) return false;
    const sc = el.querySelector<HTMLElement>("[data-slot=data-table]");
    if ((sc?.scrollTop ?? 0) > 2 && sc?.dataset.gliding !== "1") return true;
    if (el.getBoundingClientRect().top < 0) return true;
    if (hovering.current) return true;
    if (document.querySelector("[role=dialog], [data-radix-popper-content-wrapper]")) return true;
    if (el.contains(document.activeElement)) return true;
    const sel = document.getSelection();
    return !!sel && !sel.isCollapsed && el.contains(sel.anchorNode);
  }, []);

  useEffect(() => {
    const from = tail.current?.where() ?? null;
    const t = new TopicsTail(
      ask,
      pace,
      {
        onConn: setConn,
        onReady: (r) => {
          setReady(r);
          setFollowed(r.topics.map((x) => x.name));
        },
        onBatch: (b) => {
          const at = Date.now();
          setSamples((x) => [...x.filter((s) => at - s.at < 60_000), { at, counts: b.counts }]);
          setNow(at);
          setTotals((x) => ({ matched: x.matched + b.matched, scanned: x.scanned + b.scanned, skipped: x.skipped + b.skipped }));
          if (b.joined.length || b.left.length) {
            setFollowed((f) => [...f.filter((n) => !b.left.includes(n)), ...b.joined.map((j) => j.name).filter((n) => !f.includes(n))]);
            for (const j of b.joined) toast(`${j.name} joined the tail`, { description: "A new topic that matches: all of its records are new." });
            for (const l of b.left) toast(`${l} left the tail`, { description: "It was deleted, or no longer matches." });
          }
          if (b.rows.length === 0) return;
          if (isHeld()) {
            heldBuf.current = hold(heldBuf.current, b.rows, MOST_HELD);
            setHeldCount(heldBuf.current.length);
          } else {
            const all = [...b.rows, ...heldBuf.current];
            heldBuf.current = [];
            setHeldCount(0);
            setRows((r) => prepend(r, all));
          }
        },
      },
      from,
    );
    tail.current = t;
    if (pausedRef.current) t.hold("paused");
    else if (document.hidden) t.hold("hidden");
    else t.start();
    return () => t.stop();
  }, [ask, pace, isHeld]);

  // A hidden tab reads nothing; seen again, it goes on from where it was.
  useEffect(() => {
    const on = () => (document.hidden ? tail.current?.hold("hidden") : tail.current?.go("hidden"));
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);

  // The rates fall back as time passes without a batch: looked at again once a pace, only while there is a recent sample.
  const recent = samples.some((s) => now - s.at < 12_000);
  useEffect(() => {
    if (!recent || paused) return;
    const t = setInterval(() => setNow(Date.now()), Math.max(pace, 2) * 1000);
    return () => clearInterval(t);
  }, [recent, paused, pace]);
  const r = rates(samples, now, pace * 1000);
  const total = [...r.values()].reduce((a, x) => a + x.scanned, 0);
  const rateOf = (t: string) => (r.get(t)?.scanned ?? (followed.includes(t) ? 0 : null));

  const togglePause = () => {
    const t = tail.current;
    if (!t) return;
    if (pausedRef.current) {
      pausedRef.current = false;
      setPaused(false);
      t.go("paused");
    } else {
      pausedRef.current = true;
      setPaused(true);
      t.hold("paused");
    }
  };
  const show = () => {
    const sc = wrap.current?.querySelector<HTMLElement>("[data-slot=data-table]");
    if (sc) {
      sc.dataset.jump = "1";
      sc.scrollTo({ top: 0 });
      requestAnimationFrame(() => requestAnimationFrame(() => delete sc.dataset.jump));
    }
    const h = heldBuf.current;
    heldBuf.current = [];
    setHeldCount(0);
    if (h.length) setRows((x) => prepend(x, h));
  };
  const clear = () => {
    heldBuf.current = [];
    setHeldCount(0);
    setRows([]);
  };
  useKeys({ p: togglePause, n: show, "/": () => setFiltersOpen(true), c: clear });

  const hues = useMemo(() => huesOf([...(search.topic ?? []), ...followed, ...rows.map((x) => x.topic)]), [search.topic, followed, rows]);
  const wide = useWide();
  const cols = useMemo(() => columns({ open: onOpen, wide, address, hueOf: (t) => hues.get(t) ?? 1 }).map((c) => ({ ...c, enableSorting: false })), [onOpen, wide, address, hues]);
  const sample = followed[0] ?? search.topic?.[0];
  const cmd = sample ? `echo '{"order_id":"ord-1","state":"paid"}' | kcat -P -b ${shellWord(address)} -t ${shellWord(sample)} -k ord-1` : "";
  const empty = (
    <div className="flex max-w-xl flex-col items-center gap-3 text-center" data-testid="tail-waiting">
      <RadioIcon className="size-5 text-link" aria-hidden />
      <b className="text-[15px] font-semibold text-bright">{paused ? "Paused" : followed.length ? "Listening" : "Waiting for a topic"}</b>
      <p className="text-[13.5px]">
        {followed.length
          ? `New records ${search.from === "last" ? "and the last of each partition " : ""}appear here as they are written${totals.scanned > 0 && totals.matched === 0 ? ": none read so far matches the filters" : ""}.`
          : "None of these topics exists yet: each joins the tail as it is created."}
      </p>
      {cmd && (
        <div className="flex w-full items-center gap-2 rounded-lg border bg-well px-3 py-2 text-left">
          <code className="min-w-0 flex-1 truncate font-mono text-[12px] text-foreground" title={cmd}>
            {cmd}
          </code>
          <CopyButton text={cmd} what="Command copied" />
        </div>
      )}
    </div>
  );

  return (
    <div className="flex flex-col gap-4" data-testid="tail">
      <StatStrip dense stats={stats(rows.length, totals, total > 0 ? (Math.round(total * 10) / 10).toLocaleString("en-US") : "0")} />
      <FollowBar search={search} set={set} options={options} paused={paused} onPause={togglePause} filtersOpen={filtersOpen} onFiltersOpen={setFiltersOpen} />
      <ChipLine search={search} set={set} followed={followed} hues={hues} rate={rateOf} held={<HeldPill count={heldCount} onShow={show} />} conn={conn} onRetry={() => tail.current?.retry()} />
      {ready && ready.notes.length > 0 && (
        <div className="flex flex-col gap-1" role="status">
          {ready.notes.map((n) => (
            <p key={n} className="text-[13px] text-muted-foreground" data-testid="tail-note">
              {n}
            </p>
          ))}
        </div>
      )}
      <div ref={wrap} className="tail-table relative" onPointerEnter={() => (hovering.current = true)} onPointerLeave={() => (hovering.current = false)} data-testid="tail-wrap" data-tail>
        <DataTable data={rows} columns={cols} rowId={rowKey} aria-label="Records of the topics followed, newest first" maxHeight={TABLE_HEIGHT} onRowClick={onOpen} rowClassName={(x) => (x.fresh ? "row-new" : undefined)} rowData={(x) => ({ "data-hue": hues.get(x.topic) ?? 1 })} anchorTop reserve empty={empty} />
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 text-[12.5px] text-muted-foreground" data-testid="tail-foot">
        <span className="tabular-nums">
          Opened {clock(startedAt)} UTC · {search.from === "last" ? `from the last ${search.last ?? 10} of each partition` : "from then on"}
        </span>
        <span className="flex items-center gap-3">
          {[
            ["p", paused ? "resume" : "pause"],
            ["n", "show new"],
            ["/", "filters"],
            ["c", "clear the screen"],
          ].map(([k, w]) => (
            <span key={k} className="whitespace-nowrap">
              <kbd className="mr-1 rounded border bg-card px-1 font-mono text-[11px]">{k}</kbd>
              {w}
            </span>
          ))}
        </span>
      </div>
    </div>
  );
}

/** Nothing chosen: the bar (so nothing moves when a topic is), and the topics written to last as one-click starts. No stream. */
function Choose({ search, set, options, recent }: { search: TailSearch; set: (p: Patch, push?: boolean) => void; options: Option[]; recent: string[] }) {
  const hues = huesOf(recent);
  return (
    <div className="flex flex-col gap-4" data-testid="tail-choose">
      <FollowBar search={search} set={set} options={options} paused={false} onPause={() => {}} filtersOpen={false} onFiltersOpen={() => {}} />
      <EmptyState
        icon={RadioIcon}
        title="Choose what to follow"
        className="min-h-[26rem]"
        action={
          <div className="flex max-w-2xl flex-col items-center gap-4">
            {recent.length > 0 && (
              <div className="flex flex-wrap justify-center gap-2" data-testid="tail-recent">
                {recent.map((t) => (
                  <Button key={t} variant="outline" size="sm" className="bg-card font-mono text-[12.5px]" onClick={() => set({ topic: [t] }, true)} data-hue={hues.get(t) ?? 1}>
                    <span aria-hidden className="size-2 rounded-full bg-[var(--hue)]" /> {t}
                  </Button>
                ))}
              </div>
            )}
            <div className="flex flex-wrap items-center justify-center gap-2 text-[12.5px]">
              <span>Or a pattern:</span>
              {["*", "*.dlq", ...(recent[0]?.includes(".") ? [`${recent[0].split(".")[0]}.*`] : [])].map((p) => (
                <Button key={p} variant="secondary" size="sm" className="h-7 font-mono text-[12px]" onClick={() => set({ pattern: [p] }, true)}>
                  {p}
                </Button>
              ))}
            </div>
          </div>
        }
      >
        One stream of the records written to the topics you pick, by name or by a pattern like <code className="font-mono text-foreground">orders.*</code> (a topic created later that matches joins it). Filter by key, value, header or a JSON field; open any record to see all of it.
      </EmptyState>
    </div>
  );
}

export function TailPage() {
  const search = route.useSearch();
  const set = useSet();
  useTailOwnsRefresh();
  const list = useTopics("", false);
  const address = useShell().data?.address ?? "";
  const rows = list.data?.view.rows;
  const options = useMemo<Option[]>(() => (rows ?? []).map((t) => ({ value: t.name, label: t.name, hint: `${t.partitions} p` })), [rows]);
  const recent = useMemo(() => [...(rows ?? [])].filter((t) => !t.empty).sort((a, b) => (b.last_write_ms ?? 0) - (a.last_write_ms ?? 0)).slice(0, 6).map((t) => t.name), [rows]);
  const pushed = useRef(false);
  const open = useCallback(
    (r: Row) => {
      pushed.current = true;
      set({ record: `${r.topic}:${r.partition}:${r.offset}` }, true);
    },
    [set],
  );
  const close = () => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else set({ record: undefined });
  };
  const rec = recordOf(search.record);
  return (
    <>
      <PageHeader title="Live tail" sub="The records your apps write, as they arrive: one topic or many, merged newest first, filtered as they are read." />
      {following(search) ? <Following key={askQuery(search)} search={search} set={set} options={options} address={address} onOpen={open} /> : <Choose search={search} set={set} options={options} recent={recent} />}
      <RecordSheet topic={rec?.topic ?? ""} at={rec ? { partition: rec.partition, offset: rec.offset } : null} onClose={close} />
    </>
  );
}
