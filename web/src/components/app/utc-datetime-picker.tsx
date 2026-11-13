import { CalendarClockIcon, ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

const two = (n: number) => String(n).padStart(2, "0");
const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/** `2026-11-13 14:30:00 UTC`. */
export function utcText(ms: number): string {
  const d = new Date(ms);
  return `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())} UTC`;
}

/** `14:30`, `14:30:05` as seconds of the day, or null. */
export function parseClock(text: string): number | null {
  const m = /^\s*(\d{1,2}):(\d{2})(?::(\d{2}))?\s*$/.exec(text);
  if (!m) return null;
  const [h, mi, s] = [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)];
  return h < 24 && mi < 60 && s < 60 ? h * 3600 + mi * 60 + s : null;
}

/** A moment in UTC, chosen with a month grid and a time field in a Popover: never the browser's own date widget, which
 * differs in every engine and speaks the local zone (the console speaks UTC). */
export function UtcDateTimePicker({ value, onChange, className, "aria-label": ariaLabel = "Time, UTC", id }: { value: number | null; onChange: (ms: number) => void; className?: string; "aria-label"?: string; id?: string }) {
  const [open, setOpen] = useState(false);
  const [base] = useState(() => value ?? Date.now());
  const [view, setView] = useState(() => {
    const d = new Date(base);
    return { y: d.getUTCFullYear(), m: d.getUTCMonth() };
  });
  const [clock, setClock] = useState(() => {
    const d = new Date(base);
    return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
  });
  const secs = parseClock(clock);
  const chosen = value !== null ? new Date(value) : null;
  const first = new Date(Date.UTC(view.y, view.m, 1));
  const lead = (first.getUTCDay() + 6) % 7;
  const days = new Date(Date.UTC(view.y, view.m + 1, 0)).getUTCDate();
  const today = new Date();
  const pick = (day: number) => onChange(Date.UTC(view.y, view.m, day) + (secs ?? 0) * 1000);
  const step = (by: number) => setView((v) => ({ y: v.y + Math.floor((v.m + by) / 12), m: (((v.m + by) % 12) + 12) % 12 }));
  const setTime = (text: string) => {
    setClock(text);
    const s = parseClock(text);
    if (s !== null && chosen) onChange(Date.UTC(chosen.getUTCFullYear(), chosen.getUTCMonth(), chosen.getUTCDate()) + s * 1000);
  };
  const onOpen = (o: boolean) => {
    if (o && chosen) setView({ y: chosen.getUTCFullYear(), m: chosen.getUTCMonth() });
    setOpen(o);
  };
  return (
    <Popover open={open} onOpenChange={onOpen}>
      <PopoverTrigger asChild>
        <Button id={id} variant="outline" aria-label={ariaLabel} className={cn("w-72 justify-start bg-card font-mono font-normal", value === null && "text-muted-foreground", className)} data-testid="datetime-trigger">
          <CalendarClockIcon className="opacity-60" />
          <span className="truncate">{value !== null ? utcText(value) : "Choose a time, UTC"}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[19rem] p-3" align="start" data-testid="datetime-popover">
        <div className="mb-2 flex items-center justify-between">
          <Button variant="ghost" size="icon-sm" aria-label="Previous month" onClick={() => step(-1)}>
            <ChevronLeftIcon />
          </Button>
          <span className="text-[13.5px] font-medium text-bright" aria-live="polite">
            {MONTHS[view.m]} {view.y}
          </span>
          <Button variant="ghost" size="icon-sm" aria-label="Next month" onClick={() => step(1)}>
            <ChevronRightIcon />
          </Button>
        </div>
        <div className="grid grid-cols-7 gap-0.5 text-center" role="grid" aria-label={`${MONTHS[view.m]} ${view.y}`}>
          {DAYS.map((d) => (
            <span key={d} className="py-1 text-[11.5px] text-muted-foreground">
              {d}
            </span>
          ))}
          {Array.from({ length: lead }, (_, i) => (
            <span key={`x${i}`} />
          ))}
          {Array.from({ length: days }, (_, i) => {
            const day = i + 1;
            const on = chosen && chosen.getUTCFullYear() === view.y && chosen.getUTCMonth() === view.m && chosen.getUTCDate() === day;
            const isToday = today.getUTCFullYear() === view.y && today.getUTCMonth() === view.m && today.getUTCDate() === day;
            return (
              <button
                key={day}
                type="button"
                onClick={() => pick(day)}
                aria-pressed={!!on}
                aria-label={`${day} ${MONTHS[view.m]} ${view.y}`}
                className={cn(
                  "h-8 rounded-md text-[13px] tabular-nums hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none",
                  on && "bg-primary text-primary-foreground hover:bg-primary",
                  !on && isToday && "font-semibold text-primary",
                )}
              >
                {day}
              </button>
            );
          })}
        </div>
        <div className="mt-3 flex items-end gap-2">
          <div className="flex flex-1 flex-col gap-1.5">
            <Label htmlFor={`${id ?? "dt"}-clock`} className="text-[12px] text-muted-foreground">
              Time, UTC
            </Label>
            <Input id={`${id ?? "dt"}-clock`} value={clock} onChange={(e) => setTime(e.target.value)} inputMode="numeric" placeholder="14:30:00" aria-invalid={secs === null} className="font-mono" data-testid="datetime-clock" />
          </div>
          <Button size="sm" className="mb-0.5" onClick={() => setOpen(false)} disabled={value === null || secs === null} data-testid="datetime-done">
            Done
          </Button>
        </div>
        {secs === null && <p className="mt-1.5 text-[12px] text-bad">A time is hours and minutes, like 14:30 or 14:30:05.</p>}
      </PopoverContent>
    </Popover>
  );
}
