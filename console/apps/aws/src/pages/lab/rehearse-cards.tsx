import { BanIcon, CalendarClockIcon, CloudOffIcon, GaugeIcon, HourglassIcon, type LucideIcon, MoonIcon, PlugZapIcon, ServerCrashIcon, SnowflakeIcon, TimerIcon, TimerResetIcon } from "lucide-react";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import type { LabOnceKind } from "../../api/types/LabOnceKind";
import type { LabOnceTeach } from "../../api/types/LabOnceTeach";
import type { LabTeach } from "../../api/types/LabTeach";

const RULE_ICONS: Record<string, LucideIcon> = { slow: TimerIcon, throttle: GaugeIcon, unavailable: CloudOffIcon, internal: ServerCrashIcon, "access-denied": BanIcon, drop: PlugZapIcon };
const ONCE_ICONS: Record<string, LucideIcon> = { "rest-service": MoonIcon, "fire-schedule": CalendarClockIcon, "expire-visibility": HourglassIcon, "sweep-ttl": TimerResetIcon, "cold-start": SnowflakeIcon };

function Card({ id, icon: Icon, title, lede, production, children }: { id: string; icon: LucideIcon; title: string; lede: string; production: string; children: React.ReactNode }) {
  return (
    <article className="flex min-w-0 flex-col rounded-xl border bg-card p-5" data-testid={`rehearsal-${id}`} aria-label={title}>
      <div className="flex w-full items-center gap-2">
        <Icon className="size-4 shrink-0 text-warn" aria-hidden />
        <h3 className="min-w-0 text-[15px] font-semibold text-bright">{title}</h3>
      </div>
      <p className="mt-2 text-[13px] leading-5 text-muted-foreground">{lede}</p>
      <p className="mt-3 border-t pt-3 text-[12.5px] leading-[19px] text-muted-foreground">
        <b className="font-semibold text-foreground">In production: </b>
        {production}
      </p>
      <div className="mt-4 flex flex-1 items-end">{children}</div>
    </article>
  );
}

const grid = "grid gap-4 md:grid-cols-2 xl:grid-cols-3";
const loading = <div className={grid} aria-busy aria-label="Loading">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-48 rounded-xl" />)}</div>;

/** The rehearsals that are rules, from the server's lessons. Each opens the rule dialog on its kind; off while the Lab is off. */
export function RuleCards({ teach, enabled, onStart }: { teach: LabTeach[] | undefined; enabled: boolean; onStart: (tab: string) => void }) {
  if (!teach) return loading;
  return (
    <div className={grid} data-testid="rehearsal-cards">
      {teach.map((t) => (
        <Card key={t.tab} id={t.tab} icon={RULE_ICONS[t.tab] ?? GaugeIcon} title={t.title} lede={t.lede} production={t.simulates}>
          <Button type="button" variant="outline" size="sm" className="bg-card" disabled={!enabled} onClick={() => onStart(t.tab)} data-testid="rule-start">Make a rule</Button>
        </Card>
      ))}
    </div>
  );
}

/** The forced events: what each does, what it stands for, and a button that opens its form, or says why this run cannot. */
export function OnceCards({ kinds, teach, onOpen }: { kinds: LabOnceKind[] | undefined; teach: LabOnceTeach[] | undefined; onOpen: (k: LabOnceKind) => void }) {
  if (!kinds) return loading;
  return (
    <div className={grid} data-testid="event-cards">
      {kinds.map((k) => (
        <Card key={k.id} id={k.id} icon={ONCE_ICONS[k.id] ?? MoonIcon} title={k.title} lede={k.lede} production={teach?.find((t) => t.kind === k.id)?.production ?? ""}>
          <div className="grid gap-1.5">
            <Button type="button" variant="outline" size="sm" className="w-fit bg-card" disabled={!k.available} onClick={() => onOpen(k)} data-testid="once-open">Open</Button>
            {!k.available && <p className="text-[12.5px] text-muted-foreground" data-testid="once-why-not">{k.why_not}</p>}
          </div>
        </Card>
      ))}
    </div>
  );
}
