import { type ColumnDef, flexRender, getCoreRowModel, getSortedRowModel, type SortingState, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownIcon, ArrowUpIcon } from "lucide-react";
import { useRef, useState } from "react";
import { cn } from "@/lib/utils";

/** Past this many rows the body is virtual: only the rows in view are in the DOM. */
export const VIRTUAL_FROM = 500;
const ROW = 40;

export type DataColumn<T> = ColumnDef<T> & {
  /** Fixed width in px; one column may leave it out and take the rest. */
  size?: number;
  meta?: { numeric?: boolean; mono?: boolean };
};

/** A table whose layout never depends on its rows (CON-808): fixed column widths, one line per cell with an ellipsis and the
 * whole value on hover (`title`), a sticky header, stable row ids (React keys), numbers right-aligned in tabular figures,
 * a fixed row height, and a virtual body past {@link VIRTUAL_FROM} rows. */
export function DataTable<T>({ data, columns, rowId, empty, maxHeight = 560, className, "aria-label": ariaLabel, onRowClick }: { data: T[]; columns: DataColumn<T>[]; rowId: (row: T) => string; empty?: React.ReactNode; maxHeight?: number; className?: string; "aria-label"?: string; onRowClick?: (row: T) => void }) {
  const [sorting, setSorting] = useState<SortingState>([]);
  // TanStack Table returns functions that change identity on purpose; the React compiler lint knows and says so.
  // eslint-disable-next-line react-hooks/incompatible-library
  const table = useReactTable({ data, columns, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel(), getRowId: (r) => rowId(r) });
  const rows = table.getRowModel().rows;
  const virtual = rows.length > VIRTUAL_FROM;
  const scroller = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({ count: virtual ? rows.length : 0, getScrollElement: () => scroller.current, estimateSize: () => ROW, overscan: 12 });
  const items = virtual ? v.getVirtualItems() : [];
  const shown = virtual ? items.map((i) => ({ row: rows[i.index]!, at: i.index })) : rows.map((row, at) => ({ row, at }));
  const padTop = virtual && items.length ? items[0]!.start : 0;
  const padBottom = virtual && items.length ? v.getTotalSize() - items[items.length - 1]!.end : 0;
  const cols = table.getVisibleLeafColumns();
  return (
    <div ref={scroller} className={cn("relative overflow-auto rounded-xl border bg-card", className)} style={{ maxHeight }} data-slot="data-table" data-virtual={virtual || undefined}>
      <table className="w-full table-fixed border-separate border-spacing-0 text-[13.5px]" aria-label={ariaLabel} aria-rowcount={rows.length + 1}>
        <colgroup>
          {cols.map((c) => (
            <col key={c.id} style={{ width: (c.columnDef as DataColumn<T>).size ? `${(c.columnDef as DataColumn<T>).size}px` : undefined }} />
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
                        <span className="truncate">{flexRender(h.column.columnDef.header, h.getContext())}</span>
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
          {shown.map(({ row: r, at }) => (
            <tr key={r.id} id={`r-${r.id}`} data-row={r.id} aria-rowindex={at + 2} className={cn("group h-10", onRowClick && "cursor-pointer")} onClick={onRowClick ? () => onRowClick(r.original) : undefined}>
              {r.getVisibleCells().map((c) => {
                const meta = (c.column.columnDef as DataColumn<T>).meta;
                const raw = c.getValue();
                const title = typeof raw === "string" || typeof raw === "number" ? String(raw) : undefined;
                return (
                  <td key={c.id} title={title} className={cn("h-10 overflow-hidden border-b px-3 text-ellipsis whitespace-nowrap group-last:border-b-0 group-hover:bg-accent/40", meta?.numeric && "text-right tabular-nums", meta?.mono && "font-mono text-[12.5px]")}>
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
