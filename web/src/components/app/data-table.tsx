import { type ColumnDef, flexRender, getCoreRowModel, getSortedRowModel, type SortingState, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownIcon, ArrowUpIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

/** Past this many rows the body is virtual: only the rows in view are in the DOM. */
export const VIRTUAL_FROM = 500;
const ROW = 40;
const NO_SIZE = { size: undefined };
/** The least the column without a size keeps (px). */
const FILL_MIN = 120;

/** The width a header needs on one line (CON-808: a header never truncates): its uppercase words at about 9 px a letter (12 px,
 * medium, tracked), the cell padding, and the sort arrow's slot. A column the page sized narrower is widened to it, so a page
 * can never cut its own header; a header that is not text (an sr-only label) needs nothing. */
export function headerWidth(header: unknown, sortable: boolean): number {
  if (typeof header !== "string" || header.length === 0) return 0;
  return Math.ceil(header.length * 9 + 24 + (sortable ? 16 : 0));
}

export type DataColumn<T> = ColumnDef<T> & {
  /** Fixed width in px; one column may leave it out and take the rest. */
  size?: number;
  meta?: { numeric?: boolean; mono?: boolean };
};

/** A table whose layout never depends on its rows (CON-808): fixed column widths, one line per cell with an ellipsis and the
 * whole value on hover (`title`), a sticky header, stable row ids (React keys), numbers right-aligned in tabular figures,
 * a fixed row height, and a virtual body past {@link VIRTUAL_FROM} rows. */
export function DataTable<T>({ data, columns, rowId, empty, maxHeight = 560, className, "aria-label": ariaLabel, onRowClick, rowHeight = ROW, rowClassName, anchorTop = false }: { data: T[]; columns: DataColumn<T>[]; rowId: (row: T) => string; empty?: React.ReactNode; maxHeight?: number; className?: string; "aria-label"?: string; onRowClick?: (row: T) => void; /** px; 40 unless a row holds two lines. */ rowHeight?: number; /** A class per row (a new row of a tail is highlighted). */ rowClassName?: (row: T) => string | undefined; /** A tail: rows prepended at the top do not push the rows on screen (see below). */ anchorTop?: boolean }) {
  const [sorting, setSorting] = useState<SortingState>([]);
  // TanStack Table returns functions that change identity on purpose; the React compiler lint knows and says so.
  // `defaultColumn`: TanStack gives a column with no size 150 px; here no size means the column that takes the rest.
  // eslint-disable-next-line react-hooks/incompatible-library
  const table = useReactTable({ data, columns, defaultColumn: NO_SIZE as Partial<ColumnDef<T>>, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel(), getRowId: (r) => rowId(r) });
  const rows = table.getRowModel().rows;
  const virtual = rows.length > VIRTUAL_FROM;
  const scroller = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({ count: virtual ? rows.length : 0, getScrollElement: () => scroller.current, estimateSize: () => rowHeight, overscan: 12 });
  const items = virtual ? v.getVirtualItems() : [];
  const shown = virtual ? items.map((i) => ({ row: rows[i.index]!, at: i.index })) : rows.map((row, at) => ({ row, at }));
  const padTop = virtual && items.length ? items[0]!.start : 0;
  const padBottom = virtual && items.length ? v.getTotalSize() - items[items.length - 1]!.end : 0;
  // A tail prepends rows while the reader is at the top. Pushed down by the new rows, every row on screen would shift (the
  // audit measured a layout shift of 0.37 to 0.56 in Chromium). Instead the box is scrolled by the height added, in the same
  // frame (so nothing on screen moves), then glides back to the top: a scroll, not a shift, and the new rows slide in from
  // above. With reduced motion it returns at once.
  const firstId = rows[0]?.id;
  const anchor = useRef<{ id: string; top: number } | null>(null);
  useLayoutEffect(() => {
    if (!anchorTop) return;
    const sc = scroller.current;
    if (!sc) return;
    const was = anchor.current;
    // `data-jump`: the person asked for the new rows (Show): they are put in place at once.
    const jump = sc.dataset.jump === "1";
    delete sc.dataset.jump;
    if (was && firstId !== was.id && sc.scrollTop <= 2 && !jump) {
      const el = sc.querySelector<HTMLElement>(`tr[data-row="${CSS.escape(was.id)}"]`);
      const delta = el ? el.offsetTop - was.top : 0;
      if (delta > 0) {
        sc.scrollTop = delta;
        const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        requestAnimationFrame(() => sc.scrollTo({ top: 0, behavior: calm ? "auto" : "smooth" }));
      }
    }
    const top = firstId ? sc.querySelector<HTMLElement>(`tr[data-row="${CSS.escape(firstId)}"]`) : null;
    anchor.current = firstId && top ? { id: firstId, top: top.offsetTop } : null;
  }, [anchorTop, firstId]);
  const cols = table.getVisibleLeafColumns();
  const widths = cols.map((c) => {
    const size = (c.columnDef as DataColumn<T>).size;
    return size ? Math.max(size, headerWidth(c.columnDef.header, c.getCanSort())) : undefined;
  });
  // The column that takes the rest never collapses below its header (or FILL_MIN): past that the table scrolls in its box.
  const fill = cols.findIndex((_, i) => widths[i] === undefined);
  const minWidth = fill >= 0 ? widths.reduce<number>((a, w) => a + (w ?? 0), 0) + Math.max(FILL_MIN, headerWidth(cols[fill]!.columnDef.header, cols[fill]!.getCanSort())) : undefined;
  return (
    <div ref={scroller} className={cn("relative overflow-auto rounded-xl border bg-card", className)} style={{ maxHeight }} data-slot="data-table" data-virtual={virtual || undefined}>
      <table className="w-full table-fixed border-separate border-spacing-0 text-[13.5px]" style={{ minWidth }} aria-label={ariaLabel} aria-rowcount={virtual ? rows.length + 1 : undefined}>
        <colgroup>
          {cols.map((c, i) => (
            <col key={c.id} style={{ width: widths[i] !== undefined ? `${widths[i]}px` : undefined }} />
          ))}
        </colgroup>
        <thead className="sticky top-0 z-10 bg-well">
          {table.getHeaderGroups().map((g) => (
            <tr key={g.id}>
              {g.headers.map((h) => {
                const meta = (h.column.columnDef as DataColumn<T>).meta;
                const dir = h.column.getIsSorted();
                return (
                  <th key={h.id} scope="col" aria-sort={dir === "asc" ? "ascending" : dir === "desc" ? "descending" : undefined} className={cn("h-10 border-b px-3 text-left text-[12px] font-medium tracking-[0.04em] whitespace-nowrap text-muted-foreground uppercase", meta?.numeric && "text-right")}>
                    {h.column.getCanSort() ? (
                      <button type="button" onClick={h.column.getToggleSortingHandler()} className={cn("inline-flex max-w-full items-center gap-1 rounded-sm uppercase hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", meta?.numeric && "flex-row-reverse")}>
                        <span className="whitespace-nowrap">{flexRender(h.column.columnDef.header, h.getContext())}</span>
                        {dir === "asc" ? <ArrowUpIcon className="size-3" /> : dir === "desc" ? <ArrowDownIcon className="size-3" /> : <span className="size-3" />}
                      </button>
                    ) : (
                      flexRender(h.column.columnDef.header, h.getContext())
                    )}
                  </th>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody>
          {padTop > 0 && <tr aria-hidden style={{ height: padTop }} />}
          {/* aria-rowindex only when the body is virtual (rows missing from the DOM): otherwise a row prepended by a tail would
              rewrite the index of every row below it (R3, CON-1256). */}
          {shown.map(({ row: r, at }) => (
            <tr key={r.id} id={`r-${r.id}`} data-row={r.id} aria-rowindex={virtual ? at + 2 : undefined} className={cn("group", onRowClick && "cursor-pointer", rowClassName?.(r.original))} style={{ height: rowHeight }} onClick={onRowClick ? () => onRowClick(r.original) : undefined}>

              {r.getVisibleCells().map((c) => {
                const meta = (c.column.columnDef as DataColumn<T>).meta;
                const raw = c.getValue();
                const title = typeof raw === "string" || typeof raw === "number" ? String(raw) : undefined;
                return (
                  <td key={c.id} title={title} style={{ height: rowHeight }} className={cn("overflow-hidden border-b px-3 text-ellipsis whitespace-nowrap group-last:border-b-0 group-hover:bg-accent/40", meta?.numeric && "text-right tabular-nums", meta?.mono && "font-mono text-[12.5px]")}>
                    {flexRender(c.column.columnDef.cell, c.getContext())}
                  </td>
                );
              })}
            </tr>
          ))}
          {padBottom > 0 && <tr aria-hidden style={{ height: padBottom }} />}
        </tbody>
      </table>
      {rows.length === 0 && <div className="flex min-h-40 items-center justify-center p-6 text-[13.5px] text-muted-foreground">{empty ?? "Nothing to show."}</div>}
    </div>
  );
}
