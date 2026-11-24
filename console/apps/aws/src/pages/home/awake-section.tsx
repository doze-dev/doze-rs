import { Link } from "@tanstack/react-router";
import { ActivityIcon, DatabaseIcon, LayersIcon, type LucideIcon, MessagesSquareIcon, ServerCogIcon, ShieldIcon, SlidersHorizontalIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { useServices } from "../../api/queries";
import type { ServiceSeen } from "../../api/types/ServiceSeen";
import { ago } from "../../people";
import { type Awake, type AwakeGroup, awakeGroups, STATE_WORDS } from "./awake";
import { StateMark } from "./state-mark";
import { WhichServicesSheet } from "./services-sheet";

const ICONS: Record<string, LucideIcon> = {
  "Messaging and events": MessagesSquareIcon,
  "Storage and data": DatabaseIcon,
  "Compute and flow": ServerCogIcon,
  "Config and secrets": SlidersHorizontalIcon,
  Observability: ActivityIcon,
  Infrastructure: LayersIcon,
  "Identity and access": ShieldIcon,
};

/** A green dot for running, a blue moon for resting: the word says it, the mark repeats it. */
function State({ state }: { state: Awake }) {
  return (
    <span className={cn("inline-flex w-[5.75rem] shrink-0 items-center gap-1.5 text-[13px] font-semibold", state === "awake" ? "text-good" : state === "asleep" ? "text-link" : "text-muted-foreground")} data-testid="awake-state" title={state === "asleep" ? "Resting: it starts on its next call, and what is due (schedules, timers) still runs." : undefined}>
      <StateMark state={state} className={state === "awake" ? "size-2" : undefined} />
      {STATE_WORDS[state]}
    </span>
  );
}

/** When a service was last called: how long ago while that is recent, the date once it is not. */
function lastWords(lastMs: number | null, now: number): string {
  if (lastMs === null) return "Not called yet";
  return now - lastMs < 48 * 3_600_000 ? ago(lastMs, now) : dateTimeUtc(lastMs).replace(/:\d\d UTC$/, " UTC");
}

function GroupCard({ g, now }: { g: AwakeGroup; now: number }) {
  const Icon = ICONS[g.name] ?? LayersIcon;
  return (
    <section className="min-w-0 rounded-[var(--radius-card)] border bg-card px-6 pt-5 pb-3" aria-label={g.name} data-testid="awake-group" data-group={g.name}>
      <h3 className="flex items-center gap-2.5 border-b pb-3 text-[15px] font-semibold text-bright">
        <Icon className="size-[18px] text-primary" aria-hidden /> {g.name}
      </h3>
      <ul>
        {g.rows.map((r) => (
          <li key={r.name} className="flex items-center gap-3 border-b py-3 text-[13.5px] last:border-b-0" data-testid="awake-row" data-service={r.name} data-state={r.state}>
            {r.state === "off" ? (
              <span className="min-w-0 flex-1 text-muted-foreground" title="This run does not serve it (--services)">
                {r.title}
              </span>
            ) : (
              <Link to="/resources/$service" params={{ service: r.name }} className="min-w-0 flex-1 text-link hover:underline">
                {r.title}
              </Link>
            )}
            <span className="shrink-0 text-[13px] text-muted-foreground tabular-nums" data-testid="awake-last">
              {r.state === "off" ? "" : lastWords(r.lastMs, now)}
            </span>
            <State state={r.state} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** What is running: each service of the build in its group with when anything last called it. A service that gets no request for
 * `idleSecs` lets go of its memory and files and rests, so one called longer ago than that is resting. */
export function AwakeSection({ activity, idleSecs, now }: { activity: ServiceSeen[] | undefined; idleSecs: number | undefined; now: number }) {
  const services = useServices();
  const [open, setOpen] = useState(false);
  const groups = useMemo(() => (services.data && activity && idleSecs !== undefined ? awakeGroups(services.data.services, activity, idleSecs, now) : null), [services.data, activity, idleSecs, now]);
  return (
    <section className="mb-10" aria-labelledby="awake-title" data-testid="awake">
      <h2 id="awake-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
        What is running
      </h2>
      <p className="mt-1.5 max-w-[46rem] text-[13.5px] text-muted-foreground">
        Each row shows when the service was last called. A service starts on its first call and rests after {idleSecs === undefined ? "a while" : idleSecs % 60 === 0 && idleSecs >= 120 ? `${idleSecs / 60} minutes` : `${idleSecs} seconds`} without one. A resting service costs almost nothing, answers as soon as it is called, and still runs what is due.{" "}
        <button type="button" className="rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => setOpen(true)} data-testid="which-services">
          Which services run
        </button>
      </p>
      <div className="mt-4">
        {services.isError && !groups ? (
          <ErrorBlock what="The services could not be loaded" error={services.error} hint="Check that doze-aws is still running, then try again." onRetry={() => void services.refetch()} retrying={services.isFetching} testId="services-error" />
        ) : groups ? (
          <div className="grid items-start gap-6 md:grid-cols-2 xl:grid-cols-3">
            {groups.map((g) => (
              <GroupCard key={g.name} g={g} now={now} />
            ))}
          </div>
        ) : (
          <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3">
            {[0, 1, 2].map((i) => (
              <Skeleton key={i} className="h-40 rounded-[var(--radius-card)]" />
            ))}
          </div>
        )}
      </div>
      <WhichServicesSheet open={open} onOpenChange={setOpen} services={services.data?.services ?? []} />
    </section>
  );
}
