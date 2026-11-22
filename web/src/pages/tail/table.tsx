// The Live tail's merged stream as a DataTable (CON-808, CON-1583): fixed columns, a stable key per record, the topic's colour
// on each row; the headers a click away, a menu per row to open the record or copy it as a command that writes it again.
import { useQueryClient } from "@tanstack/react-query";
import { CopyIcon, EllipsisIcon, PanelRightOpenIcon, TagsIcon, TerminalIcon } from "lucide-react";
import { recordQuery } from "@/api/topics";
import type { Shown } from "@/api/types/Shown";
import { copyText } from "@/components/app/copy";
import type { DataColumn } from "@/components/app/data-table";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { number } from "@/lib/format";
import { clockMs, dateTime } from "@/lib/kafka";
import { cn } from "@/lib/utils";
import { STANDING } from "../topics/words";
import { commands, lineOf, type Row } from "./stream";

/** A key or a value in one line: a schema's language and id first, when it has one. */
export function Cell({ s, what }: { s: Shown; what: "key" | "value" }) {
  if (s.schema) {
    const k = s.schema;
    return (
      <span className="inline-flex max-w-full min-w-0 items-baseline gap-2" data-testid="schema-cell">
        <span className="shrink-0 rounded bg-accent px-1.5 font-sans text-[11px] text-link" title={k.subject ? `${k.subject} v${k.version}, id ${k.id}` : `schema id ${k.id}`}>
          {k.kind ?? "id"} · {k.id}
        </span>
        <span className={cn("truncate", k.error ? "text-muted-foreground" : "text-foreground")}>{k.preview}</span>
      </span>
    );
  }
  if (s.kind === "null") return <span className="text-muted-foreground">null{what === "value" && " (deletion)"}</span>;
  if (s.kind === "empty") return <span className="text-muted-foreground">empty</span>;
  return <span className={s.kind === "binary" ? "text-muted-foreground" : what === "value" ? "text-foreground" : "text-bright"}>{lineOf(s)}</span>;
}

function Headers({ r }: { r: Row }) {
  if (r.headers_total === 0) return <span className="text-muted-foreground">none</span>;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 gap-1.5 px-2 font-mono text-[12px]" onClick={(e) => e.stopPropagation()} aria-label={`${r.headers_total} headers`} data-testid="row-headers">
          <TagsIcon className="size-3.5 text-muted-foreground" /> {number(r.headers_total)}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[min(30rem,calc(100vw-2rem))] p-0" onClick={(e) => e.stopPropagation()}>
        <div className="border-b px-3.5 py-2.5 text-[12.5px] font-semibold text-bright">
          Headers <span className="font-normal text-muted-foreground">of {r.topic}-{r.partition} · {number(r.offset)}</span>
        </div>
        <dl className="grid max-h-72 grid-cols-[minmax(6rem,auto)_minmax(0,1fr)] gap-x-4 gap-y-1.5 overflow-auto px-3.5 py-3 font-mono text-[12px]" data-testid="headers-list">
          {r.headers.map((h, i) => (
            <div key={`${h.key}-${i}`} className="contents">
              <dt className="truncate text-muted-foreground" title={h.key}>
                {h.key}
              </dt>
              <dd className="min-w-0 break-all text-bright">{h.value.kind === "null" ? <span className="text-muted-foreground">null</span> : lineOf(h.value)}</dd>
            </div>
          ))}
        </dl>
        {r.headers_total > r.headers.length && <p className="border-t px-3.5 py-2 text-[12px] text-muted-foreground">The first {r.headers.length} of {number(r.headers_total)}: the record has them all.</p>}
      </PopoverContent>
    </Popover>
  );
}

function RowMenu({ r, address, onOpen }: { r: Row; address: string; onOpen: (r: Row) => void }) {
  const qc = useQueryClient();
  const copy = async (as: "kcat" | "producer" | "value") => {
    const v = await qc.fetchQuery(recordQuery(r.topic, { partition: r.partition, offset: r.offset })).catch(() => null);
    if (!v) {
      await copyText("", "The record is no longer there");
      return;
    }
    if (as === "value") {
      await copyText(v.value.text, "Value copied");
      return;
    }
    const c = commands(v, address);
    if ("why" in c) {
      const { toast } = await import("sonner");
      toast.error("Not as a command", { description: c.why });
      return;
    }
    await copyText(as === "kcat" ? c.kcat : c.producer, as === "kcat" ? "kcat command copied" : "Console producer command copied");
  };
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon-sm" className="size-7 text-muted-foreground" aria-label={`More for ${r.topic}-${r.partition} at ${r.offset}`} onClick={(e) => e.stopPropagation()} data-testid="row-menu">
          <EllipsisIcon />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72" onClick={(e) => e.stopPropagation()}>
        <DropdownMenuLabel className="font-mono text-[12px] font-normal text-muted-foreground">
          {r.topic}-{r.partition} · {number(r.offset)}
        </DropdownMenuLabel>
        <DropdownMenuItem onSelect={() => onOpen(r)}>
          <PanelRightOpenIcon /> Open the record
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void copy("kcat")} data-testid="copy-kcat">
          <TerminalIcon /> Copy as a kcat command
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void copy("producer")} data-testid="copy-producer">
          <TerminalIcon /> Copy as kafka-console-producer
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void copy("value")} disabled={r.value.kind === "null"}>
          <CopyIcon /> Copy the value
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The columns at a width: under 1280 px the Headers column goes (the menu and the Sheet have them) and Key narrows. */
export function columns({ open, wide, address, hueOf }: { open: (r: Row) => void; wide: boolean; address: string; hueOf: (t: string) => number }): DataColumn<Row>[] {
  const all: DataColumn<Row>[] = [
    {
      id: "topic",
      accessorKey: "topic",
      header: "Topic",
      size: wide ? 176 : 150,
      cell: ({ row: { original: r } }) => (
        <span data-hue={hueOf(r.topic)} className="inline-flex max-w-full min-w-0 items-center gap-2 font-mono text-[12.5px] text-bright">
          <span aria-hidden className="size-2 shrink-0 rounded-full bg-[var(--hue)]" />
          <span className="truncate">{r.topic}</span>
        </span>
      ),
    },
    {
      id: "at",
      accessorFn: (r) => `partition ${r.partition}, offset ${r.offset}`,
      header: "Offset",
      size: wide ? 128 : 116,
      meta: { mono: true },
      cell: ({ row: { original: r } }) => (
        <button
          type="button"
          className="inline-flex items-baseline gap-1.5 rounded-sm text-link tabular-nums hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
          onClick={(e) => {
            e.stopPropagation();
            open(r);
          }}
          data-testid="open-record"
        >
          <span className="text-[11.5px] text-muted-foreground">p{r.partition}</span>
          {number(r.offset)}
        </button>
      ),
    },
    {
      id: "time",
      accessorFn: (r) => dateTime(r.timestamp_ms),
      header: "Time (UTC)",
      size: 122,
      meta: { mono: true },
      cell: ({ row: { original: r } }) => <span className="text-muted-foreground">{clockMs(r.timestamp_ms)}</span>,
    },
    { id: "key", accessorFn: (r) => lineOf(r.key), header: "Key", size: wide ? 168 : 120, meta: { mono: true }, cell: ({ row: { original: r } }) => <Cell s={r.key} what="key" /> },
    {
      id: "value",
      accessorFn: (r) => lineOf(r.value),
      header: "Value",
      meta: { mono: true },
      cell: ({ row: { original: r } }) => (
        <span className="inline-flex max-w-full min-w-0 items-baseline gap-2">
          {r.standing !== "committed" && <span className={cn("shrink-0 rounded px-1.5 font-sans text-[11px]", r.standing === "above_lso" ? "bg-warn/10 text-warn" : "bg-accent text-muted-foreground")}>{STANDING[r.standing].word}</span>}
          <span className="truncate">
            <Cell s={r.value} what="value" />
          </span>
        </span>
      ),
    },
    { id: "headers", accessorFn: (r) => r.headers.map((h) => `${h.key}: ${lineOf(h.value)}`).join("\n"), header: "Headers", size: 104, cell: ({ row: { original: r } }) => <Headers r={r} /> },
    { id: "menu", header: () => <span className="sr-only">Actions</span>, size: 52, enableSorting: false, cell: ({ row: { original: r } }) => <RowMenu r={r} address={address} onOpen={open} /> },
  ];
  return wide ? all : all.filter((c) => c.id !== "headers");
}
