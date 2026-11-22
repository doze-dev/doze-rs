import { useMemo } from "react";
import type { CallRecord } from "../../api/types/CallRecord";
import { number } from "@doze/ui/lib/format";
import { perMinute } from "./graph";

const W = 300;
const H = 34;

/** The newest minute the record's calls span, and how many calls each minute held: the calls the page holds, not three hours of
 * history (the record is the newest calls, in memory). Draws bars, newest at the right. */
export function Timeline({ calls, now, minutes = 30 }: { calls: readonly CallRecord[]; now: number; minutes?: number }) {
  const counts = useMemo(() => perMinute(calls, now, minutes), [calls, now, minutes]);
  const peak = Math.max(1, ...counts);
  const held = calls.length === 0 ? 0 : Math.max(1, Math.ceil((now - (calls[calls.length - 1]?.at_ms ?? now)) / 60_000));
  const bar = W / minutes;
  return (
    <div className="flex flex-col items-end" data-testid="timeline">
      <p className="mb-1 flex items-center gap-2 text-[12.5px] text-muted-foreground">
        <span>
          {number(calls.length)} {calls.length === 1 ? "call" : "calls"} held{held > 0 ? `, the last ${held} min` : ""}
        </span>
      </p>
      <svg width={W} height={H} role="img" aria-label={`Calls per minute over the last ${minutes} minutes: the busiest minute had ${number(Math.max(...counts))}`} className="overflow-visible">
        {counts.map((n, i) => (
          <rect key={i} x={i * bar + 0.5} y={H - Math.max(n > 0 ? 2 : 0, (n / peak) * H)} width={Math.max(1, bar - 1.5)} height={Math.max(n > 0 ? 2 : 0, (n / peak) * H)} rx={1} fill="var(--primary)" opacity={i === minutes - 1 ? 1 : 0.55} />
        ))}
      </svg>
      <div className="mt-0.5 flex w-full justify-between text-[11px] text-muted-foreground">
        <span>{minutes} min ago</span>
        <span>now</span>
      </div>
    </div>
  );
}
