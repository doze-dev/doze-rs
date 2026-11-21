import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { cn } from "@doze/ui/lib/utils";

/** Two or three choices in one pill, one chosen (`Relative | Absolute`, `UTC | Local time zone`): a toggle group, so a keyboard
 * moves along it and a reader hears one choice, not several checkboxes. Choosing the chosen one again changes nothing. */
export function Segmented<T extends string>({ value, onChange, options, label, className, testId }: { value: T; onChange: (v: T) => void; options: readonly { value: T; label: string }[]; label: string; className?: string; testId?: string }) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => v && onChange(v as T)} aria-label={label} className={cn("gap-0.5 rounded-lg border bg-muted p-0.5", className)} data-testid={testId}>
      {options.map((o) => (
        <ToggleGroupItem key={o.value} value={o.value} className="h-8 rounded-md px-3.5 text-[13.5px] font-normal text-muted-foreground data-[state=on]:bg-card data-[state=on]:font-medium data-[state=on]:text-bright data-[state=on]:shadow-xs" data-testid={`${testId ?? "segmented"}-${o.value}`}>
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
