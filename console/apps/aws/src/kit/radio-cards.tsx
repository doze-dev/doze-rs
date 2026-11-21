import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { Label } from "@doze/ui/components/ui/label";
import { cn } from "@doze/ui/lib/utils";

export type RadioCard<T extends string> = { value: T; title: React.ReactNode; body?: React.ReactNode };

/** Choices as cards side by side, each with a title and what it means: the board's radio cards (a log class, a capacity mode). The
 * card chosen is outlined; the radio inside is the control, so it is a radio group to a keyboard and a reader. */
export function RadioCards<T extends string>({ value, onChange, options, label, name, disabled = false, className }: { value: T; onChange: (v: T) => void; options: readonly RadioCard<T>[]; label: string; /** Prefix of the test ids and element ids (`class` gives `class-standard`). */ name: string; disabled?: boolean; className?: string }) {
  return (
    <RadioGroup value={value} onValueChange={(v) => onChange(v as T)} className={cn("grid gap-3 sm:grid-cols-2", className)} aria-label={label} disabled={disabled}>
      {options.map((o) => (
        <Label key={o.value} htmlFor={`${name}-${o.value}`} className="flex cursor-pointer items-start gap-3 rounded-xl border bg-card p-4 font-normal leading-5 has-[[data-state=checked]]:border-primary has-[[data-state=checked]]:bg-pick/40">
          <RadioGroupItem id={`${name}-${o.value}`} value={o.value} className="mt-0.5" data-testid={`${name}-${o.value}`} />
          <span className="grid gap-0.5">
            <span className="font-medium text-bright">{o.title}</span>
            {o.body && <span className="text-[13px] text-muted-foreground">{o.body}</span>}
          </span>
        </Label>
      ))}
    </RadioGroup>
  );
}
