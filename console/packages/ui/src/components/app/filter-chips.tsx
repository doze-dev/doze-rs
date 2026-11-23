import { cn } from "../../lib/utils";
import { type Chip, chipValue, chipWords } from "./filter-chips-state";

/** A row of filter chips with their counts (`All 12`, `In alarm 2`): one is chosen. Controlled: `value` is the chosen chip's
 * value (a page keeps it in the address with `useUrlParam`). A chip whose count is 0 stays: a filter that vanishes when it is
 * empty is a filter that moves under the cursor. */
export function FilterChips({ chips, value, onChange, label, className }: { chips: Chip[]; value: string | undefined; onChange: (value: string) => void; /** Names the group: `Filter alarms by state`. */ label: string; className?: string }) {
  const chosen = chipValue(value, chips);
  return (
    <div role="radiogroup" aria-label={label} className={cn("flex flex-wrap items-center gap-2", className)} data-slot="filter-chips">
      {chips.map((c) => {
        const on = c.value === chosen;
        return (
          <button
            key={c.value}
            type="button"
            role="radio"
            aria-checked={on}
            aria-label={chipWords(c)}
            onClick={() => onChange(c.value)}
            data-testid={`chip-${c.value || "all"}`}
            className={cn("inline-flex h-8 items-center gap-2 rounded-full border px-3.5 text-[13px] whitespace-nowrap transition-colors outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50", on ? "border-primary/50 bg-pick font-medium text-bright" : "bg-card text-foreground hover:bg-accent")}
          >
            {c.label}
            {c.count !== undefined && c.count !== null && <span className={cn("text-[12px] tabular-nums", on ? "text-foreground" : "text-muted-foreground")}>{c.count.toLocaleString("en-US")}</span>}
          </button>
        );
      })}
    </div>
  );
}
