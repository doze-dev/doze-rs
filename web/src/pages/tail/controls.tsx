// The controls of the Live tail page (CON-1580 to CON-1584): what to follow (names and patterns), where to start, the filters
// the server applies, Pause, the speed, the connection, and the chips that say what is on (each topic with its colour and its
// rate). One layout that never moves: every slot that comes and goes is reserved.
import { FilterIcon, PauseIcon, PlayIcon, XIcon } from "lucide-react";
import { useEffect, useState } from "react";
import type { Option } from "@/components/app/combobox";
import { MultiSelect } from "@/components/app/multi-select";
import { TailSpeed } from "@/components/app/tail-controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { LAST_CHOICES, type TailSearch, filterChips, validPath } from "./search";
import { type Conn, rateText } from "./stream";

export type Patch = Partial<TailSearch>;

/** A topic's chip: its colour, its name, and (while tailing) its rate. */
export function TopicChip({ name, hue, rate, waiting, onRemove, className }: { name: string; hue: number; rate?: string; waiting?: boolean; onRemove?: () => void; className?: string }) {
  return (
    <span data-hue={hue} className={cn("inline-flex h-7 max-w-full min-w-0 items-center gap-1.5 rounded-full border bg-[color-mix(in_oklab,var(--hue)_9%,var(--card))] pr-1 pl-2.5 font-mono text-[12px] text-bright", waiting && "border-dashed bg-card text-muted-foreground", !onRemove && "pr-2.5", className)} data-testid="topic-chip" data-topic={name}>
      <span aria-hidden className="size-2 shrink-0 rounded-full bg-[var(--hue)]" />
      <span className="truncate" title={name}>
        {name}
      </span>
      {rate !== undefined && (
        <span className="inline-block min-w-[6ch] text-right text-muted-foreground tabular-nums" title="Records a second written to it, over the last 10 s" data-testid="topic-rate">
          {rate}/s
        </span>
      )}
      {waiting && <span className="text-[11px]">waiting</span>}
      {onRemove && (
        <button type="button" aria-label={`Stop following ${name}`} onClick={onRemove} className="rounded-full p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
          <XIcon className="size-3" />
        </button>
      )}
    </span>
  );
}

function toneOf(conn: Conn | null, now: number): [string, string] {
  if (!conn) return ["bg-muted-foreground", "Not following"];
  switch (conn.state) {
    case "live":
      return ["bg-good", "Live"];
    case "connecting":
      return ["bg-warn", "Connecting…"];
    case "reconnecting":
      return ["bg-warn", "Reconnecting…"];
    case "paused":
      return ["bg-muted-foreground", "Paused"];
    case "hidden":
      return ["bg-muted-foreground", "Paused · tab hidden"];
    case "disconnected":
      return ["bg-bad", `Disconnected · Retry in ${Math.max(1, Math.ceil((conn.retryAt - now) / 1000))} s`];
  }
}

/** The connection, in a slot of reserved width: Live, Connecting, Reconnecting, Paused, Disconnected (with the seconds to
 * the next try and a Retry). */
export function ConnState({ conn, onRetry }: { conn: Conn | null; onRetry: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  const counting = conn?.state === "disconnected";
  useEffect(() => {
    if (!counting) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [counting]);
  const [tone, word] = toneOf(conn, now);
  return (
    <span className="inline-flex min-w-[13.5rem] items-center justify-end gap-2 text-[13px]" data-testid="tail-conn" data-state={conn?.state ?? "off"} role="status" title={conn?.state === "disconnected" ? conn.why : undefined}>
      <span aria-hidden className={cn("size-2 shrink-0 rounded-full", tone, conn?.state === "live" && "shadow-[0_0_0_3px] shadow-good/20")} />
      <span className={cn("font-medium whitespace-nowrap tabular-nums", conn?.state === "live" ? "text-bright" : "text-muted-foreground")}>{word}</span>
      {conn?.state === "disconnected" && (
        <Button variant="link" size="sm" className="h-auto px-1" onClick={onRetry} data-testid="tail-retry">
          Retry now
        </Button>
      )}
    </span>
  );
}

type Draft = { key: string; value: string; header: string; header_value: string; field: string; equals: string; partition: string; committed: boolean };

function draftOf(s: TailSearch): Draft {
  return { key: s.key ?? "", value: s.value ?? "", header: s.header ?? "", header_value: s.header_value ?? "", field: s.field ?? "", equals: s.equals ?? "", partition: (s.partition ?? []).join(", "), committed: s.isolation === "read_committed" };
}

function patchOf(d: Draft): Patch {
  const t = (v: string) => v.trim() || undefined;
  const parts = [...new Set(d.partition.split(/[\s,]+/).filter((x) => /^\d{1,6}$/.test(x)).map(Number))].sort((a, b) => a - b);
  const header = t(d.header);
  const field = t(d.field);
  return { key: t(d.key), value: t(d.value), header, header_value: header ? t(d.header_value) : undefined, field, equals: field ? t(d.equals) : undefined, partition: parts.length ? parts : undefined, isolation: d.committed ? "read_committed" : undefined };
}

function Field({ id, label, hint, bad, children }: { id: string; label: string; hint?: string; bad?: boolean; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <Label htmlFor={id} className="text-[12.5px]">
        {label}
      </Label>
      {children}
      {hint && <p className={cn("text-[11.5px]", bad ? "text-bad" : "text-muted-foreground")}>{hint}</p>}
    </div>
  );
}

/** The filters, applied by the server while it reads (so a match is never missed for being off screen): a Popover form. */
export function FiltersButton({ search, set, open, onOpenChange }: { search: TailSearch; set: (p: Patch) => void; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [d, setD] = useState<Draft>(() => draftOf(search));
  const on = filterChips(search).length;
  const bad = !validPath(d.field);
  const field = (k: keyof Draft, id: string, placeholder: string) => (
    <Input id={id} value={d[k] as string} onChange={(e) => setD({ ...d, [k]: e.target.value })} placeholder={placeholder} className="h-8 font-mono text-[12.5px]" autoComplete="off" spellCheck={false} maxLength={256} aria-invalid={k === "field" && bad ? true : undefined} />
  );
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        if (o) setD(draftOf(search));
        onOpenChange(o);
      }}
    >
      <PopoverTrigger asChild>
        <Button variant="outline" className={cn("bg-card", on > 0 && "border-primary/50 text-bright")} data-testid="tail-filters">
          <FilterIcon /> Filters
          <span className={cn("inline-flex min-w-5 justify-center rounded-full bg-primary px-1.5 text-[11px] leading-[18px] font-semibold text-primary-foreground tabular-nums", on === 0 && "invisible")}>{on}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[min(36rem,calc(100vw-2rem))]">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (bad) return;
            set(patchOf(d));
            onOpenChange(false);
          }}
        >
          <div>
            <h3 className="text-[14px] font-semibold text-bright">Keep only the records that match</h3>
            <p className="mt-0.5 text-[12.5px] text-muted-foreground">The console applies these as it reads, so the counts say how many matched of how many were written. Letters of either case match.</p>
          </div>
          <div className="grid gap-x-4 gap-y-3 sm:grid-cols-2">
            <Field id="f-key" label="Key contains">
              {field("key", "f-key", "ord-1")}
            </Field>
            <Field id="f-value" label="Value contains" hint="A value with a schema is matched as its JSON.">
              {field("value", "f-value", "declined")}
            </Field>
            <Field id="f-header" label="Header named">
              {field("header", "f-header", "trace-id")}
            </Field>
            <Field id="f-hv" label="Its value contains">
              <Input id="f-hv" value={d.header_value} onChange={(e) => setD({ ...d, header_value: e.target.value })} placeholder={d.header ? "any value" : "name a header first"} disabled={!d.header.trim()} className="h-8 font-mono text-[12.5px]" autoComplete="off" spellCheck={false} maxLength={256} />
            </Field>
            <Field id="f-field" label="JSON field" hint="A path like customer.id or items[0].sku." bad={bad}>
              {field("field", "f-field", "customer.id")}
            </Field>
            <Field id="f-eq" label="Equals" hint="Empty: the field is there.">
              <Input id="f-eq" value={d.equals} onChange={(e) => setD({ ...d, equals: e.target.value })} placeholder={d.field ? "c-7, 42, true" : "name a field first"} disabled={!d.field.trim()} className="h-8 font-mono text-[12.5px]" autoComplete="off" spellCheck={false} maxLength={256} />
            </Field>
            <Field id="f-part" label="Partitions" hint="Of every topic; empty: all.">
              {field("partition", "f-part", "0, 2")}
            </Field>
            <div className="flex min-w-0 flex-col gap-1.5">
              <span className="text-[12.5px] font-medium" id="f-iso">
                Reading as
              </span>
              <ToggleGroup type="single" variant="outline" size="sm" value={d.committed ? "c" : "u"} onValueChange={(v) => v && setD({ ...d, committed: v === "c" })} aria-labelledby="f-iso" className="bg-card">
                <ToggleGroupItem value="u" className="px-2.5 font-mono text-[12px]">
                  read_uncommitted
                </ToggleGroupItem>
                <ToggleGroupItem value="c" className="px-2.5 font-mono text-[12px]">
                  read_committed
                </ToggleGroupItem>
              </ToggleGroup>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button type="submit" size="sm" disabled={bad} data-testid="filters-apply">
              Apply
            </Button>
            <Button type="button" size="sm" variant="outline" onClick={() => setD(draftOf({}))}>
              Clear the form
            </Button>
            <span className="ml-auto text-[12px] text-muted-foreground">
              <kbd className="rounded border px-1 font-mono text-[11px]">/</kbd> opens this
            </span>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}

/** Row one: what to follow, where to start, the filters; at the right Pause and the speed. */
export function FollowBar({ search, set, options, paused, onPause, filtersOpen, onFiltersOpen }: { search: TailSearch; set: (p: Patch, push?: boolean) => void; options: Option[]; paused: boolean; onPause: () => void; filtersOpen: boolean; onFiltersOpen: (o: boolean) => void }) {
  const chosen = [...(search.topic ?? []), ...(search.pattern ?? [])];
  const pick = (v: string[]) => {
    const names = v.filter((x) => !/[*?]/.test(x));
    const pats = v.filter((x) => /[*?]/.test(x));
    set({ topic: names.length ? names : undefined, pattern: pats.length ? pats : undefined }, true);
  };
  const following = chosen.length > 0;
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2" data-testid="tail-bar">
      <MultiSelect options={options} value={chosen} onChange={pick} noun="topics" none="Choose topics to follow" aria-label="Topics to follow" chips={false} pattern triggerClassName="w-56 xl:w-64" testId="tail-topics" />
      <Select value={search.from === "last" ? String(search.last ?? 10) : "now"} onValueChange={(v) => set(v === "now" ? { from: undefined, last: undefined } : { from: "last", last: Number(v) })}>
        <SelectTrigger className="w-[12.5rem] bg-card" aria-label="Where the tail starts" data-testid="tail-start">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="now" title="Only what is written from the moment the tail opens">
            From now
          </SelectItem>
          {LAST_CHOICES.map((n) => (
            <SelectItem key={n} value={String(n)} title={`The last ${n} records of every partition, then what is written`}>
              Last {n} per partition
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <FiltersButton key={JSON.stringify(filterChips(search).map((c) => c.id))} search={search} set={set} open={filtersOpen} onOpenChange={onFiltersOpen} />
      <span className="flex-1" />
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant={paused ? "default" : "outline"} className={cn("w-[7.25rem]", !paused && "bg-card")} onClick={onPause} disabled={!following} data-testid="tail-pause">
            {paused ? <PlayIcon /> : <PauseIcon />} {paused ? "Resume" : "Pause"}
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {paused ? "Read on from where it paused" : "Stop reading; Resume goes on from here"} <kbd className="ml-1 rounded border px-1 font-mono text-[11px]">p</kbd>
        </TooltipContent>
      </Tooltip>
      <TailSpeed />
    </div>
  );
}

/** Row two: the topics followed (each with its colour and rate; a name not created yet waits), the filters on, and at the right
 * the held pill's reserved slot and the connection. */
export function ChipLine({ search, set, followed, hues, rate, held, conn, onRetry }: { search: TailSearch; set: (p: Patch, push?: boolean) => void; followed: string[]; hues: Map<string, number>; rate: (topic: string) => number | null; held: React.ReactNode; conn: Conn | null; onRetry: () => void }) {
  const names = search.topic ?? [];
  const patterns = search.pattern ?? [];
  const extra = followed.filter((t) => !names.includes(t));
  const chips = filterChips(search);
  const drop = (t: string) => {
    const left = names.filter((x) => x !== t);
    set({ topic: left.length ? left : undefined }, true);
  };
  return (
    <div className="flex min-h-9 flex-wrap items-center justify-between gap-x-4 gap-y-2" data-testid="tail-chips">
      <div className="flex min-w-0 flex-1 basis-[24rem] flex-wrap items-center gap-1.5">
        {names.map((t) => {
          const r = rate(t);
          return <TopicChip key={t} name={t} hue={hues.get(t) ?? 1} waiting={!followed.includes(t)} rate={followed.includes(t) && r !== null ? rateText(r) : undefined} onRemove={() => drop(t)} />;
        })}
        {patterns.map((p) => (
          <Badge key={p} variant="outline" className="h-7 gap-1 rounded-full bg-card pr-1 pl-2.5 font-mono text-[12px]" data-testid="pattern-chip">
            <span className="text-muted-foreground">matching</span> {p}
            <button type="button" aria-label={`Stop following ${p}`} onClick={() => set({ pattern: patterns.filter((x) => x !== p).length ? patterns.filter((x) => x !== p) : undefined }, true)} className="rounded-full p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
              <XIcon className="size-3" />
            </button>
          </Badge>
        ))}
        {extra.map((t) => {
          const r = rate(t);
          return <TopicChip key={t} name={t} hue={hues.get(t) ?? 1} rate={r !== null ? rateText(r) : undefined} />;
        })}
        {chips.length > 0 && <span aria-hidden className="mx-1 h-5 w-px bg-border" />}
        {chips.map((c) => (
          <Badge key={c.id} variant="secondary" className="h-7 gap-1 pr-1 pl-2.5 font-mono text-[12px]" data-testid={`filter-chip-${c.id}`}>
            <span className="max-w-[18rem] truncate" title={c.label}>
              {c.label}
            </span>
            <button type="button" aria-label={`Remove ${c.label}`} onClick={() => set(c.off)} className="rounded-full p-0.5 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
              <XIcon className="size-3" />
            </button>
          </Badge>
        ))}
      </div>
      <div className="flex items-center gap-3">
        {held}
        <ConnState conn={conn} onRetry={onRetry} />
      </div>
    </div>
  );
}
