// The Config tab of a topic (board topic-config): every config the broker lists, grouped, with its value, where it comes
// from, Kafka's first sentence and when a change counts. The filter and the chips are in the address; "Where from" and
// "Kafka's text" open in place (Collapsibles: no navigation, the scroll stays). Edit and Reset are Dialogs routed by
// `?edit=` and `?reset=`. A snapshot with no refresh control (live-design.md: Config is "Snapshot, no control").
import { useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { CheckIcon, ChevronDownIcon, InfoIcon, MinusIcon, MoreHorizontalIcon, SearchIcon, SlidersHorizontalIcon, XIcon } from "lucide-react";
import { lazy, Suspense, useMemo, useRef, useState } from "react";
import type { ConfigRowView } from "@/api/types/ConfigRowView";
import type { ConfigTab as Tab } from "@/api/types/ConfigTab";
import { useConfigTab } from "@/api/topic-writes";
import { EmptyState } from "@/components/app/empty-state";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import type { ConfigSearch } from "@/lib/search";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";

const ConfigDialogs = lazy(() => import("./config-dialogs"));

type Show = ConfigSearch["show"];

const COLS = "grid grid-cols-[minmax(0,46fr)_minmax(0,19fr)_minmax(0,15fr)_minmax(0,14fr)] gap-x-4";

function matches(r: ConfigRowView, q: string, show: Show): boolean {
  const t = q.toLowerCase();
  const byQ = !t || r.name.toLowerCase().includes(t) || r.doc.toLowerCase().includes(t);
  if (!byQ) return false;
  if (show === "changed") return r.changed;
  if (show === "compaction") return r.group === "cleanup";
  if (show === "segments") return r.group === "segments";
  return true;
}

function SourceBadge({ r }: { r: ConfigRowView }) {
  return (
    <span className={cn("inline-flex w-fit items-center rounded-md border px-1.5 py-0.5 font-mono text-[11.5px] whitespace-nowrap", r.source === "default" ? "border-dashed text-muted-foreground" : "border-primary/30 bg-primary/5 text-link")} data-testid="source">
      {r.source_label}
    </span>
  );
}

function PanelButton({ open, onClick, children, controls }: { open: boolean; onClick: () => void; children: React.ReactNode; controls: string }) {
  return (
    <button type="button" aria-expanded={open} aria-controls={controls} onClick={onClick} className="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[12.5px] font-medium text-link hover:bg-primary/5 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
      <ChevronDownIcon aria-hidden className={cn("size-3.5 transition-transform duration-150", open ? "rotate-0" : "-rotate-90")} />
      {children}
    </button>
  );
}

function Row({ r, onEdit, onReset }: { r: ConfigRowView; onEdit: (k: string) => void; onReset: (k: string) => void }) {
  const [where, setWhere] = useState(false);
  const [text, setText] = useState(false);
  const id = r.name.replace(/[^a-zA-Z0-9-]/g, "-");
  return (
    <li className="border-b last:border-b-0" data-testid="config-row" data-key={r.name} id={`c-${id}`}>
      <div className={cn(COLS, "items-start px-4 py-3")}>
        <div className="min-w-0">
          <div className="font-mono text-[13px] font-medium break-all text-bright" data-testid="config-name">
            {r.name}
          </div>
          {r.doc_first && <p className="mt-0.5 text-[12.5px] leading-[18px] text-muted-foreground">{r.doc_first}</p>}
          {r.effect && <p className="mt-0.5 text-[12.5px] leading-[18px] text-muted-foreground" data-testid="effect">{r.effect}</p>}
          {(r.changed || r.doc_more) && (
            <div className="mt-1.5 -ml-1.5 flex flex-wrap gap-1">
              {r.changed && r.places.length > 0 && (
                <PanelButton open={where} onClick={() => setWhere((o) => !o)} controls={`where-${id}`}>
                  Where from
                </PanelButton>
              )}
              {r.doc_more && (
                <PanelButton open={text} onClick={() => setText((o) => !o)} controls={`text-${id}`}>
                  Kafka's text
                </PanelButton>
              )}
            </div>
          )}
        </div>
        <div className="min-w-0">
          <div className="font-mono text-[13px] break-all text-foreground" data-testid="config-value">
            {r.value}
          </div>
          {r.gloss && <div className="font-mono text-[11.5px] text-muted-foreground">{r.gloss}</div>}
        </div>
        <div className="min-w-0">
          <SourceBadge r={r} />
        </div>
        <div className="flex justify-end gap-1.5">
          {r.on_topic ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm" aria-label={`Actions for ${r.name}`} data-testid="row-menu">
                  Edit <MoreHorizontalIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => onEdit(r.name)}>Edit</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => onReset(r.name)}>Reset to the inherited value</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <Button variant="outline" size="sm" onClick={() => onEdit(r.name)} aria-label={`Edit ${r.name}`} data-testid="row-edit">
              Edit
            </Button>
          )}
        </div>
      </div>
      <Collapsible open={where}>
        <CollapsibleContent id={`where-${id}`} className="mx-4 mb-3 rounded-lg border-l-2 border-primary bg-primary/5 p-4 text-[13px]" data-testid="where-from">
          <p>
            <b className="font-semibold text-bright">Where {r.name} comes from.</b> The first place with a value wins. DescribeConfigs lists them when synonyms are asked for.
          </p>
          <ol className="mt-3 flex flex-col gap-1.5">
            {r.places.map((p) => (
              <li key={p.title} className="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] items-start gap-2">
                {p.wins ? <CheckIcon aria-label="applies" className="mt-0.5 size-4 text-good" /> : <MinusIcon aria-hidden className="mt-0.5 size-4 text-muted-foreground" />}
                <span className="min-w-0">
                  <span className={p.wins ? "font-medium text-bright" : "text-foreground"}>{p.title}</span>
                  <span className="block font-mono text-[11.5px] break-all text-muted-foreground">{p.detail}</span>
                </span>
                <code className="font-mono text-[12px] text-foreground">{p.value}</code>
              </li>
            ))}
          </ol>
          {r.takes_over && <p className="mt-3 text-muted-foreground">{r.takes_over}</p>}
        </CollapsibleContent>
      </Collapsible>
      <Collapsible open={text}>
        <CollapsibleContent id={`text-${id}`} className="mx-4 mb-3 rounded-lg border-l-2 border-primary bg-primary/5 p-4 text-[13px]" data-testid="kafka-text">
          <p>
            <b className="font-semibold text-bright">Kafka's text for {r.name}</b>, as DescribeConfigs returns it when documentation is asked for.
          </p>
          <p className="mt-3 leading-[21px] text-foreground">{r.doc}</p>
          <dl className="mt-3 grid grid-cols-[6rem_minmax(0,1fr)] gap-x-4 gap-y-1">
            <dt className="text-muted-foreground">Type</dt>
            <dd className="font-mono">{r.type_word}</dd>
            <dt className="text-muted-foreground">Allowed</dt>
            <dd>{r.allowed || "–"}</dd>
            <dt className="text-muted-foreground">Default</dt>
            <dd className="font-mono break-all">{r.default ?? "none"}</dd>
            <dt className="text-muted-foreground">Here</dt>
            <dd className="font-mono break-all">
              {r.value} <span className="font-sans text-muted-foreground">{r.source_label}</span>
            </dd>
          </dl>
        </CollapsibleContent>
      </Collapsible>
    </li>
  );
}

function Section({ id, title, lede, rows, onEdit, onReset, testId }: { id: string; title: string; lede: string; rows: ConfigRowView[]; onEdit: (k: string) => void; onReset: (k: string) => void; testId?: string }) {
  return (
    <section className="mb-9" aria-labelledby={`sec-${id}`} data-testid={testId ?? `section-${id}`}>
      <h2 id={`sec-${id}`} className="flex items-center gap-2 text-[17px] font-semibold text-bright">
        {title} <span className="rounded-full bg-well px-2 py-0.5 text-[12px] font-medium text-muted-foreground tabular-nums">{rows.length}</span>
      </h2>
      {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
      {rows.length > 0 && (
        <div className="mt-3 overflow-hidden rounded-xl border bg-card">
          <div className={cn(COLS, "border-b bg-well px-4 py-2 text-[11.5px] font-medium tracking-[0.05em] text-muted-foreground uppercase")} aria-hidden>
            <span>Config</span>
            <span>Value</span>
            <span>Source</span>
            <span />
          </div>
          <ul>
            {rows.map((r) => (
              <Row key={r.name} r={r} onEdit={onEdit} onReset={onReset} />
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

function Replication({ rows, lede, onEdit, onReset }: { rows: ConfigRowView[]; lede: string; onEdit: (k: string) => void; onReset: (k: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <Collapsible open={open} onOpenChange={setOpen} className="mb-9" data-testid="section-replication">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-[17px] font-semibold text-bright">
            Replication and tiered storage <span className="rounded-full bg-well px-2 py-0.5 text-[12px] font-medium text-muted-foreground tabular-nums">{rows.length}</span>
          </h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>
        </div>
        <CollapsibleTrigger asChild>
          <Button variant="outline" size="sm" data-testid="replication-toggle">
            {open ? "Hide" : "Show"} {rows.length}
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent>
        <div className="mt-3 overflow-hidden rounded-xl border bg-card">
          <ul>
            {rows.map((r) => (
              <Row key={r.name} r={r} onEdit={onEdit} onReset={onReset} />
            ))}
          </ul>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function Sections({ tab, q, show, onEdit, onReset }: { tab: Tab; q: string; show: Show; onEdit: (k: string) => void; onReset: (k: string) => void }) {
  const shown = tab.rows.filter((r) => matches(r, q, show));
  const changed = shown.filter((r) => r.changed);
  const filtered = !!q || !!show;
  return (
    <>
      <Section
        id="changed"
        title="Changed from Kafka's default"
        lede={filtered || changed.length > 0 ? "Set here or by a flag. These are the ones worth reading first." : "Nothing is set on this topic and no flag differs from Kafka's defaults."}
        rows={changed}
        onEdit={onEdit}
        onReset={onReset}
      />
      {tab.groups.map((g) => {
        const rows = shown.filter((r) => !r.changed && r.group === g.id);
        if (rows.length === 0) return null;
        if (g.id === "replication") return <Replication key={g.id} rows={rows} lede={g.lede} onEdit={onEdit} onReset={onReset} />;
        return <Section key={g.id} id={g.id} title={g.title} lede={g.lede} rows={rows} onEdit={onEdit} onReset={onReset} />;
      })}
      {shown.length === 0 && (
        <EmptyState icon={SlidersHorizontalIcon} title="No config matches">
          Clear the filter to see all {number(tab.rows.length)}.
        </EmptyState>
      )}
    </>
  );
}

/** The route component of `/topics/$topic/config` (R1a's frame around it). */
export function TopicConfigPage() {
  const { topic } = useParams({ from: "/topics/$topic/config" });
  return <TopicConfigTab topic={topic} />;
}

/** The tab's body, under the topic's frame. */
export function TopicConfigTab({ topic }: { topic: string }) {
  const search = useSearch({ from: "/topics/$topic/config" });
  const navigate = useNavigate({ from: "/topics/$topic/config" });
  const data = useConfigTab(topic);
  const [typed, setTyped] = useState(search.q ?? "");
  // The address changed under the field (Back, a link): the field follows it.
  const [lastQ, setLastQ] = useState(search.q);
  if (lastQ !== search.q) {
    setLastQ(search.q);
    if ((search.q ?? "") !== typed.trim()) setTyped(search.q ?? "");
  }
  const [ever, setEver] = useState(false);
  if ((search.edit || search.reset) && !ever) setEver(true);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const set = (patch: Partial<ConfigSearch>, replace = true) => void navigate({ search: (s: ConfigSearch) => ({ ...s, ...patch }), replace, resetScroll: false });
  // A dialog opened from this page pushed an entry: closing goes Back. Opened by a deep link, closing replaces.
  const pushed = useRef(false);
  const openDialog = (patch: Partial<ConfigSearch>) => {
    pushed.current = true;
    set(patch, false);
  };
  const closeDialog = () => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else set({ edit: undefined, reset: undefined });
  };
  const counts = useMemo(() => (data.data ? { all: data.data.rows.length, changed: data.data.rows.filter((r) => r.changed).length } : null), [data.data]);
  if (data.error && !data.data) return <PageError error={data.error} onRetry={() => void data.refetch()} />;
  const tab = data.data;
  return (
    <div data-testid="config-tab">
      <p className="mb-5 max-w-[56rem] text-[14px] leading-[22px] text-muted-foreground" data-testid="config-sentence">
        {tab ? <Spans spans={tab.sentence} /> : <Skeleton className="h-5 w-2/3" />}
      </p>
      <div className="mb-5 flex flex-wrap items-center gap-3" role="search">
        <div className="relative w-full max-w-sm">
          <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={typed}
            onChange={(e) => {
              const v = e.target.value;
              setTyped(v);
              clearTimeout(timer.current);
              timer.current = setTimeout(() => set({ q: v.trim() || undefined }), 200);
            }}
            placeholder="Filter by name or by what it does"
            aria-label="Filter configs"
            className="pr-9 pl-9"
            data-testid="config-filter"
          />
          {typed && (
            <button
              type="button"
              aria-label="Clear the filter"
              className="absolute top-1/2 right-2 grid size-6 -translate-y-1/2 place-items-center rounded-md text-muted-foreground hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
              onClick={() => {
                setTyped("");
                set({ q: undefined });
              }}
            >
              <XIcon className="size-3.5" />
            </button>
          )}
        </div>
        <ToggleGroup type="single" variant="outline" value={search.show ?? "all"} onValueChange={(v) => v && set({ show: v === "all" ? undefined : (v as Show) }, false)} aria-label="Show" data-testid="config-chips">
          <ToggleGroupItem value="all" className="gap-1.5 px-3">
            All <span className="text-muted-foreground tabular-nums">{counts ? number(counts.all) : ""}</span>
          </ToggleGroupItem>
          <ToggleGroupItem value="changed" className="gap-1.5 px-3">
            Changed <span className="text-muted-foreground tabular-nums">{counts ? number(counts.changed) : ""}</span>
          </ToggleGroupItem>
          <ToggleGroupItem value="compaction" className="px-3">
            Compaction
          </ToggleGroupItem>
          <ToggleGroupItem value="segments" className="px-3">
            Segments
          </ToggleGroupItem>
        </ToggleGroup>
      </div>
      <div className="mb-7 flex gap-3 rounded-xl border bg-card p-4 text-[13.5px] leading-[21px] text-muted-foreground">
        <InfoIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-cool" />
        <p>
          <b className="font-semibold text-bright">Where a value comes from, first match wins:</b> set on this topic (Dynamic topic), then set for this broker or all brokers (Dynamic broker), then a start flag that differs from Kafka's default (Static flag), then Kafka's default. Open <b className="font-semibold text-foreground">Where from</b> on any changed row to see every place.
        </p>
      </div>
      {tab ? (
        <Sections tab={tab} q={search.q ?? ""} show={search.show} onEdit={(k) => openDialog({ edit: k, reset: undefined })} onReset={(k) => openDialog({ reset: k, edit: undefined })} />
      ) : (
        <div className="flex flex-col gap-3">
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
        </div>
      )}
      {ever && (
        <Suspense fallback={null}>
          <ConfigDialogs topic={topic} edit={search.edit} reset={search.reset} onClose={closeDialog} />
        </Suspense>
      )}
    </div>
  );
}
