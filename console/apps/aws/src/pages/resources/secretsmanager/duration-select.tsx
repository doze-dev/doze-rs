import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";

/** The rotation windows the console offers: hours, as Secrets Manager's `Duration` is written (`3h`). One a secret already has
 * (made with the CLI) is offered too. */
export const WINDOWS = ["1h", "2h", "3h", "4h", "6h", "8h", "12h", "24h"] as const;

const NONE = "none";

/** The window's length, chosen from the hours (the board's select), or none. */
export function DurationSelect({ id, value, onChange }: { id: string; value: string; onChange: (v: string) => void }) {
  const kept = value.trim();
  const options = kept !== "" && !(WINDOWS as readonly string[]).includes(kept) ? [kept, ...WINDOWS] : WINDOWS;
  return (
    <Select value={kept === "" ? NONE : kept} onValueChange={(v) => onChange(v === NONE ? "" : v)}>
      <SelectTrigger id={id} className="w-40 bg-card font-mono" data-testid="rotation-duration">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NONE}>Not set</SelectItem>
        {options.map((w) => (
          <SelectItem key={w} value={w} className="font-mono">
            {w}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
