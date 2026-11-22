import { CheckIcon, MinusIcon, XIcon } from "lucide-react";
import { useMemo } from "react";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { type DataColumn, DataTable } from "./data-table";
import { allState, selectedWords, toggle, toggleAll } from "./selection";

/** The header's checkbox: a tick when every row is chosen, a bar when some are (the shared Checkbox draws a tick only). */
function HeaderCheck({ state, onClick, label }: { state: "none" | "some" | "all"; onClick: () => void; label: string }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={state === "all" ? true : state === "some" ? "mixed" : false}
      aria-label={label}
      onClick={onClick}
      data-state={state === "none" ? "unchecked" : state === "all" ? "checked" : "indeterminate"}
      className="grid size-4 place-content-center rounded-[4px] border border-input bg-card align-middle shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=indeterminate]:border-primary data-[state=indeterminate]:bg-primary data-[state=indeterminate]:text-primary-foreground"
      data-testid="select-all"
    >
      {state === "all" ? <CheckIcon className="size-3.5" /> : state === "some" ? <MinusIcon className="size-3.5" /> : null}
    </button>
  );
}

/** A row's radio, where one row at a time is chosen (`single`): a ring with a dot, like the radios of a form. */
function RowRadio({ on, onClick, label, id }: { on: boolean; onClick: () => void; label: string; id: string }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      aria-label={label}
      onClick={onClick}
      data-state={on ? "checked" : "unchecked"}
      className="grid size-4 place-content-center rounded-full border border-input bg-card align-middle shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 data-[state=checked]:border-primary"
      data-testid={`select-${id}`}
    >
      {on && <span className="size-2 rounded-full bg-primary" />}
    </button>
  );
}

/** A DataTable with a checkbox on every row: "N selected" and the bulk actions above it. The selection is the caller's (a set
 * of row ids), so a page can keep it in the address or clear it after a delete. The bar's height is reserved, so choosing a row
 * moves nothing. Clicking a checkbox does not open the row. */
export function SelectableTable<T>({ data, columns, rowId, selected, onSelectedChange, actions, noun = "row", hint, toolbar, rowClassName, actionsEnd = false, actionsAlways = false, selectedLabel, single = false, ...table }: Omit<React.ComponentProps<typeof DataTable<T>>, "columns"> & { columns: DataColumn<T>[]; selected: ReadonlySet<string>; onSelectedChange: (next: Set<string>) => void; /** The bulk actions, given the chosen rows (a Delete button). */ actions?: (chosen: T[]) => React.ReactNode; /** `parameter`: names the checkboxes (`Select parameter x`). */ noun?: string; /** What the bar says when nothing is chosen (not when there is a toolbar: that fills the bar). */ hint?: string; /** The list's own controls (a search field, FilterChips) at the left of the bar: the count and the bulk actions come to its right, so the bar is one row. */ toolbar?: React.ReactNode; /** The bulk actions are drawn with nothing chosen too (given no rows, so a page can show them off). */ actionsAlways?: boolean; /** The bulk actions at the right end of the bar, across from the count, instead of beside it. */ actionsEnd?: boolean; /** The count's words (`1 selected`) instead of `1 message selected`. */ selectedLabel?: (count: number) => string; /** One row at a time, chosen with a radio (the list's actions are for one): choosing another row moves the choice, and there is no select-all. */ single?: boolean }) {
  const ids = useMemo(() => data.map(rowId), [data, rowId]);
  const state = allState(selected, ids);
  const chosen = data.filter((r) => selected.has(rowId(r)));
  const select: DataColumn<T> = {
    id: "select",
    size: 44,
    enableSorting: false,
    header: () => (single ? <span className="sr-only">Choose one {noun}</span> : <HeaderCheck state={state} onClick={() => onSelectedChange(toggleAll(selected, ids))} label={state === "all" ? `Clear every ${noun}` : `Select every ${noun}`} />),
    cell: (c) => {
      const id = rowId(c.row.original);
      return (
        <span onClick={(e) => e.stopPropagation()} className="flex">
          {single ? <RowRadio on={selected.has(id)} onClick={() => onSelectedChange(new Set([id]))} label={`Select ${noun} ${id}`} id={id} /> : <Checkbox checked={selected.has(id)} onCheckedChange={() => onSelectedChange(toggle(selected, id))} aria-label={`Select ${noun} ${id}`} data-testid={`select-${id}`} />}
        </span>
      );
    },
  };
  return (
    <div className="grid gap-3" data-slot="selectable-table">
      <span className="sr-only" aria-live="polite" data-testid="selection-live">
        {chosen.length > 0 ? selectedWords(chosen.length, noun) : ""}
      </span>
      <div className={cn("flex min-h-9 flex-wrap items-center gap-3 text-[13.5px]", chosen.length === 0 && "text-muted-foreground")} data-testid="selection-bar">
        {toolbar && <div className="flex min-w-0 flex-1 flex-wrap items-center gap-3">{toolbar}</div>}
        {chosen.length === 0 ? (
          <>
            {!toolbar && <span>{hint ?? `Choose ${noun}s to act on them together.`}</span>}
            {actionsAlways && actions && <div className={cn("flex flex-wrap items-center gap-2", actionsEnd && "ml-auto")}>{actions([])}</div>}
          </>
        ) : (
          <>
            <span className={cn("font-medium text-bright", !toolbar && "mr-1")} data-testid="selected-count">
              {selectedLabel ? selectedLabel(chosen.length) : selectedWords(chosen.length, noun)}
            </span>
            <Button type="button" variant="ghost" size="sm" onClick={() => onSelectedChange(new Set())} data-testid="selection-clear">
              <XIcon /> Clear
            </Button>
            {actions && <div className={cn("flex flex-wrap items-center gap-2", actionsEnd && "ml-auto")}>{actions(chosen)}</div>}
          </>
        )}
      </div>
      <DataTable<T> {...table} data={data} rowId={rowId} columns={[select, ...columns]} rowClassName={(r) => cn(selected.has(rowId(r)) && "[&>td]:bg-pick/50", rowClassName?.(r))} />
    </div>
  );
}
