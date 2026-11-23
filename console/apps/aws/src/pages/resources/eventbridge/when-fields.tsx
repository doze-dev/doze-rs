// A moment as the boards draw it: a date field, a time field and the zone (eventbridge__replay-start). The console speaks UTC, so the zone is fixed.
import { useState } from "react";
import { parseClock } from "@doze/ui/components/app/utc-datetime-picker";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";

const two = (n: number) => String(n).padStart(2, "0");

/** `2026-11-07` as the start of that day in UTC (ms), or null when it is not a real date. */
export function parseDay(text: string): number | null {
  const m = /^\s*(\d{4})-(\d{2})-(\d{2})\s*$/.exec(text);
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const at = Date.UTC(y, mo - 1, d);
  const back = new Date(at);
  return back.getUTCFullYear() === y && back.getUTCMonth() === mo - 1 && back.getUTCDate() === d ? at : null;
}

/** The date and the time of day, in UTC, of a moment in ms. */
export function splitUtc(ms: number | null): { day: string; clock: string } {
  if (ms === null) return { day: "", clock: "" };
  const d = new Date(ms);
  return { day: `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())}`, clock: `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}` };
}

/** The moment a date and a time of day, in UTC, say (ms); null when either is not valid. */
export function joinUtc(day: string, clock: string): number | null {
  const at = parseDay(day);
  const secs = parseClock(clock);
  return at === null || secs === null ? null : at + secs * 1000;
}

/** Date, time and zone for one moment: `onChange` is given the moment, or null while what is typed is not a date and a time. */
export function WhenFields({ label, value, onChange, testId }: { label: string; value: number | null; onChange: (ms: number | null) => void; testId: string }) {
  const [text, setText] = useState(() => splitUtc(value));
  const set = (p: Partial<typeof text>) => {
    const next = { ...text, ...p };
    setText(next);
    onChange(joinUtc(next.day, next.clock));
  };
  const badDay = text.day !== "" && parseDay(text.day) === null;
  const badClock = text.clock !== "" && parseClock(text.clock) === null;
  return (
    <div className="grid gap-1.5" data-testid={testId}>
      <Label htmlFor={`${testId}-day`}>{label}</Label>
      <div className="flex flex-wrap items-center gap-2">
        <Input id={`${testId}-day`} value={text.day} onChange={(e) => set({ day: e.target.value })} placeholder="2026-11-07" inputMode="numeric" autoComplete="off" aria-label={`${label}, date`} aria-invalid={badDay} className="w-36 font-mono" data-testid={`${testId}-date`} />
        <Input value={text.clock} onChange={(e) => set({ clock: e.target.value })} placeholder="00:00:00" inputMode="numeric" autoComplete="off" aria-label={`${label}, time`} aria-invalid={badClock} className="w-28 font-mono" data-testid={`${testId}-time`} />
        {/* The console speaks UTC: the zone is a choice of one, drawn as the board draws it. */}
        <Select value="UTC"><SelectTrigger className="w-24 bg-card font-mono" aria-label={`${label}, zone`} data-testid={`${testId}-zone`}><SelectValue /></SelectTrigger><SelectContent><SelectItem value="UTC" className="font-mono">UTC</SelectItem></SelectContent></Select>
      </div>
      {(badDay || badClock) && <p className="text-[12.5px] text-bad" role="alert">{badDay ? "A date is year-month-day, like 2026-11-07." : "A time is hours and minutes, like 14:30 or 14:30:05."}</p>}
    </div>
  );
}
