// The range of a log search: Relative (a span back from now) or Absolute (two moments, UTC), as the boards draw it.
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Segmented } from "../../../kit";
import { RANGES, type Range } from "./range";

export function RangeControls({ range, onChange }: { range: Range; onChange: (r: Range) => void }) {
  return (
    <>
      <Segmented
        label="Kind of time range"
        testId="range-mode"
        value={range.mode}
        onChange={(m) => onChange(m === "relative" ? { mode: "relative", id: "1h" } : { mode: "absolute", from: Date.now() - 3_600_000, to: Date.now() })}
        options={[{ value: "relative", label: "Relative" }, { value: "absolute", label: "Absolute" }]}
      />
      {range.mode === "relative" ? (
        <Select value={range.id} onValueChange={(id) => onChange({ mode: "relative", id })}>
          <SelectTrigger className="h-9 w-44 border-0 bg-transparent px-1 text-[14px] text-muted-foreground shadow-none" aria-label="Relative range" data-testid="range-select"><SelectValue /></SelectTrigger>
          <SelectContent>{RANGES.map((r) => <SelectItem key={r.id} value={r.id}>{r.label}</SelectItem>)}</SelectContent>
        </Select>
      ) : (
        <span className="flex flex-wrap items-center gap-2" data-testid="range-absolute">
          <UtcDateTimePicker value={range.from} onChange={(ms) => onChange({ ...range, from: ms })} aria-label="From, UTC" className="w-60" />
          <span className="text-[13px] text-muted-foreground">to</span>
          <UtcDateTimePicker value={range.to} onChange={(ms) => onChange({ ...range, to: ms })} aria-label="To, UTC" className="w-60" />
        </span>
      )}
    </>
  );
}
