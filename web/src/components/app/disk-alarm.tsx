// The heads-up when the disk needs someone (data_words.rs decides when and says it): on Home and on the Data page.
import { OctagonAlertIcon, TriangleAlertIcon } from "lucide-react";
import type { DiskAlarm } from "@/api/types/DiskAlarm";
import { cn } from "@/lib/utils";

export function DiskAlarmBanner({ alarm, action }: { alarm: DiskAlarm; action?: React.ReactNode }) {
  const bad = alarm.tone === "bad";
  const Icon = bad ? OctagonAlertIcon : TriangleAlertIcon;
  return (
    <div role="status" className={cn("flex gap-3 rounded-xl border p-5", bad ? "border-bad/35 bg-bad/5" : "border-warn/35 bg-warn/5")} data-testid="disk-alarm" data-tone={alarm.tone}>
      <Icon aria-hidden className={cn("mt-0.5 size-5 shrink-0", bad ? "text-bad" : "text-warn")} />
      <div className="flex min-w-0 flex-1 flex-wrap items-center justify-between gap-x-6 gap-y-3">
        <div className="min-w-0 flex-1 basis-80 text-[14px] leading-[21px]">
          <b className="font-semibold text-bright" data-testid="disk-alarm-headline">
            {alarm.headline}
          </b>
          <p className="mt-1 text-muted-foreground">{alarm.detail}</p>
        </div>
        {action && <div className="flex flex-none items-center">{action}</div>}
      </div>
    </div>
  );
}
