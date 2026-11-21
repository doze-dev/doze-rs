import { type ColumnDef, flexRender, getCoreRowModel, getSortedRowModel, type SortingState, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownIcon, ArrowUpIcon } from "lucide-react";
import { useLayoutEffect, useRef, useState } from "react";
import { cn } from "../../lib/utils";

/** Past this many rows the body is virtual: only the rows in view are in the DOM. */
export const VIRTUAL_FROM = 500;
/** The row height (px) before the theme's is known, and without a theme. */
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
  /** px: when the box is narrower than the table, a sized column gives up width down to this (never below its header) before
   * the table scrolls in its box; the column that takes the rest keeps at least this. */
  min?: number;
  meta?: { numeric?: boolean; mono?: boolean };
};

/** The widths of the columns (px; `undefined` for the one that takes the rest) and the least the table may be, in a box `box` px
 * wide (0: not known yet). A sized column is at least its header; the column that takes the rest at least its header, its
 * `min` and {@link FILL_MIN}. When they add up to more than the box, the sized columns with a `min` give up width, each in
 * proportion to what it can spare, down to their `min`; past that the table scrolls in its box. */
export function columnWidths(cols: { size?: number; min?: number; header: number }[], box: number): { widths: (number | undefined)[]; minWidth: number | undefined } {
  const widths = cols.map((c) => (c.size ? Math.max(c.size, c.header) : undefined));
  const fill = widths.findIndex((w) => w === undefined);
  const fillMin = fill >= 0 ? Math.max(FILL_MIN, cols[fill]!.header, cols[fill]!.min ?? 0) : 0;
  const total = widths.reduce<number>((a, w) => a + (w ?? 0), 0) + fillMin;
  if (box > 0 && total > box) {
    const spare = cols.map((c, i) => (widths[i] !== undefined && c.min !== undefined ? Math.max(0, widths[i]! - Math.max(c.min, c.header)) : 0));
    const room = spare.reduce((a, b) => a + b, 0);
    if (room > 0) {
      const give = Math.min(room, total - box);
      spare.forEach((sp, i) => {
        if (sp > 0) widths[i] = Math.floor(widths[i]! - (give * sp) / room);
      });
    }
  }
  if (fill < 0) return { widths, minWidth: undefined };
  return { widths, minWidth: widths.reduce<number>((a, w) => a + (w ?? 0), 0) + fillMin };
}

/** A table whose layout never depends on its rows (CON-808): fixed column widths, one line per cell with an ellipsis and the
 * whole value on hover (`title`), a sticky header, stable row ids (React keys), numbers right-aligned in tabular figures,
 * a fixed row height, and a virtual body past {@link VIRTUAL_FROM} rows. */
export function DataTable<T>({ data, columns, rowId, empty, maxHeight = 560, className, "aria-label": ariaLabel, onRowClick, rowHeight: ownRowHeight, rowClassName, anchorTop = false }: { data: T[]; columns: DataColumn<T>[]; rowId: (row: T) => string; empty?: React.ReactNode; maxHeight?: number; className?: string; "aria-label"?: string; onRowClick?: (row: T) => void; /** px; the theme's `--row-h` unless a row holds two lines. */ rowHeight?: number; /** A class per row (a new row of a tail is highlighted). */ rowClassName?: (row: T) => string | undefined; /** A tail: rows prepended at the top do not push the rows on screen (see below). */ anchorTop?: boolean }) {
  const [sorting, setSorting] = useState<SortingState>([]);
  // TanStack Table returns functions that change identity on purpose; the React compiler lint knows and says so.
  // `defaultColumn`: TanStack gives a column with no size 150 px; here no size means the column that takes the rest.
  // eslint-disable-next-line react-hooks/incompatible-library
  const table = useReactTable({ data, columns, defaultColumn: NO_SIZE as Partial<ColumnDef<T>>, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel(), getRowId: (r) => rowId(r) });
  const rows = table.getRowModel().rows;
  const virtual = rows.length > VIRTUAL_FROM;
  const scroller = useRef<HTMLDivElement>(null);
  // Density is the theme's (`--row-h`, in px): read where the table is, so a denser product needs no other table.
  const [themeRow, setThemeRow] = useState(ROW);
  useLayoutEffect(() => {
    const px = scroller.current ? Number.parseFloat(getComputedStyle(scroller.current).getPropertyValue("--row-h")) : Number.NaN;
    if (Number.isFinite(px) && px > 0) setThemeRow(px);
  }, []);
  const rowHeight = ownRowHeight ?? themeRow;
  const v = useVirtualizer({ count: virtual ? rows.length : 0, getScrollElement: () => scroller.current, estimateSize: () => rowHeight, overscan: 12 });
  // The virtual rows were placed with the height known at first; when the theme's arrives they are placed again.
  useLayoutEffect(() => v.measure(), [v, rowHeight]);
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
  // The box's width (not the rows'): the layout still never depends on what the rows hold.
  const [box, setBox] = useState(0);
  useLayoutEffect(() => {
    const sc = scroller.current;
    if (!sc || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setBox(sc.clientWidth));
    seen.observe(sc);
    return () => seen.disconnect();
  }, []);
  const cols = table.getVisibleLeafColumns();
  const { widths, minWidth } = columnWidths(
    cols.map((c) => {
      const def = c.columnDef as DataColumn<T>;
      return { size: def.size, min: def.min, header: headerWidth(c.columnDef.header, c.getCanSort()) };
    }),
    box,
  );
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
