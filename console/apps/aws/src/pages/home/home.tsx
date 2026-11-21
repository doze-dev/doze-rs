import { Link, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { CheckCircle2Icon, Link2Icon, RadioIcon } from "lucide-react";
import { useCallback, useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { clockMsUtc, number, plural } from "@doze/ui/lib/format";
import { useAbout } from "../../api/queries";
import { useArrivals, useHome, useHomeChecks } from "../../api/spaces";
import type { CallRecord } from "../../api/types/CallRecord";
import type { CallerSeen } from "../../api/types/CallerSeen";
import type { KeyPrincipal } from "../../api/types/KeyPrincipal";
import { ago, clientName, who } from "../../people";
import { iamWords } from "../../words";
import { Result } from "../live/outcome";
import { Card, CardSkeleton, CopyRow, Section, useNow } from "../card";
import { BuildServices } from "./services";
import { Signals } from "./signals";

const HINT = "Check that doze-aws is still running, then try again. Nothing was changed.";

function Connected({ callers, now }: { callers: CallerSeen[]; now: number }) {
  return (
    <ul className="grid" data-testid="connected-callers">
      {callers.slice(0, 4).map((c) => {
        const w = who(c.principal);
        return (
          <li key={c.principal.key} className="flex items-center gap-4 border-t py-3.5 first:border-t-0 first:pt-0" data-testid="connected-caller" data-key={c.principal.key}>
            <div className="min-w-0 flex-1">
              <StatusDot tone="ok">
                <span title={w.title}>Heard from {w.label}</span>
              </StatusDot>
              <div className="mt-1 truncate text-[12.5px] text-muted-foreground" data-testid="caller-sdks">
                {c.sdks.length ? c.sdks.join(" · ") : "No client named in its User-Agent"} · {plural(c.calls, "call")}
              </div>
            </div>
            <span className="shrink-0 text-[13px] text-muted-foreground">{ago(c.last_ms, now)}</span>
          </li>
        );
      })}
      {callers.length > 4 && <li className="border-t pt-3 text-[13px] text-muted-foreground">and {number(callers.length - 4)} more in Live</li>}
    </ul>
  );
}

function recentColumns(names: ReadonlyMap<string, KeyPrincipal>): DataColumn<CallRecord>[] {
  return [
    { id: "time", header: "Time", size: 130, min: 122, accessorKey: "at_ms", meta: { mono: true }, cell: (c) => clockMsUtc(c.getValue<number>()) },
    {
      id: "caller",
      header: "Caller",
      size: 200,
      min: 140,
      accessorFn: (r) => r.caller ?? "",
      cell: (c) => {
        const key = c.getValue<string>();
        const p = names.get(key);
        const w = p ? who(p) : { label: key, title: key };
        return (
          <span title={w.title}>
            {w.label}
            {c.row.original.sdk && <span className="ml-1.5 text-[12px] text-muted-foreground">({clientName(c.row.original.sdk)})</span>}
          </span>
        );
      },
    },
    { id: "action", header: "Operation", size: 190, min: 140, accessorFn: (r) => r.action ?? "", meta: { mono: true } },
    { id: "resource", header: "Resource", accessorFn: (r) => r.resource ?? "", meta: { mono: true } },
    { id: "result", header: "Result", size: 220, min: 140, enableSorting: false, accessorFn: (r) => r.outcome, cell: (c) => <Result call={c.row.original} /> },
  ];
}

/** Home: where an app connects and whether one has, who it is and with which SDK (from the Live record), what needs a look
 * (from the record, and from the services apps called), the newest calls, and the services of the build. */
export function HomePage() {
  const about = useAbout();
  const home = useHome();
  const checks = useHomeChecks();
  const qc = useQueryClient();
  const navigate = useNavigate();
  const now = useNow();
  const onCalls = useCallback(() => void qc.invalidateQueries({ queryKey: ["home"], exact: true }), [qc]);
  useArrivals(onCalls);
  const { refetch: refetchHome } = home;
  const { refetch: refetchChecks } = checks;
  const refresh = useCallback(() => {
    void refetchHome();
    void refetchChecks();
  }, [refetchHome, refetchChecks]);
  const a = about.data;
  const h = home.data;
  const names = useMemo(() => new Map((h?.callers ?? []).map((c) => [c.principal.key, c.principal])), [h]);
  const columns = useMemo(() => recentColumns(names), [names]);
  return (
    <>
      <PageHeader
        title="Home"
        sub={
          a ? (
            <span data-testid="home-sub">
              account <span className="font-mono text-foreground">{a.account}</span> · <span className="font-mono text-foreground">{a.region}</span> · {iamWords(a.iam).chip} · doze-aws <span className="font-mono text-foreground">{a.version}</span>
            </span>
          ) : (
            "This doze-aws: where your app connects, and what it serves."
          )
        }
        refresh={<RefreshControl updatedAt={home.dataUpdatedAt} fetching={home.isFetching || checks.isFetching} onRefresh={refresh} />}
        actions={
          <Button asChild>
            <Link to="/connect" data-testid="home-connect">
              <Link2Icon /> Connect your app
            </Link>
          </Button>
        }
      />

      {about.isError && !a && <ErrorBlock className="mb-8" what="What this doze-aws is could not be loaded" error={about.error} hint={HINT} onRetry={() => void about.refetch()} retrying={about.isFetching} testId="about-error" />}

      <div className="mb-10 grid gap-6 lg:grid-cols-2">
        <Card testId="card-connect" title="Where your app connects" lede="Every service answers on this one address. Point an AWS SDK or the AWS CLI at it.">
          {a ? (
            // grid-cols-1: a track of minmax(0, 1fr), so a long value is cut with an ellipsis instead of pushing Copy out of the card.
            <div className="grid grid-cols-1">
              <CopyRow label="Endpoint" value={a.endpoint} testId="endpoint" />
              <CopyRow label="As an environment variable" value={`AWS_ENDPOINT_URL=${a.endpoint}`} testId="endpoint-env" />
            </div>
          ) : (
            <CardSkeleton rows={2} />
          )}
        </Card>
        <Card
          testId="card-connected"
          title="Is my app connected?"
          lede="Ticks when a request from an app reaches doze-aws (this console's own calls do not count), and says which client sent it."
        >
          {home.isError && !h ? (
            <ErrorBlock what="The Live record could not be read" error={home.error} hint={HINT} onRetry={() => void home.refetch()} retrying={home.isFetching} testId="home-error" />
          ) : !h ? (
            <CardSkeleton rows={2} />
          ) : h.connected ? (
            <div data-testid="connected" data-state="yes">
              <Connected callers={h.callers} now={now} />
            </div>
          ) : (
            <div data-testid="connected" data-state="no">
              <StatusDot tone="idle">Waiting for the first call</StatusDot>
              <p className="mt-2 text-[13px] text-muted-foreground">Nothing has called since doze-aws started. This turns over by itself when a call arrives.</p>
            </div>
          )}
          <div className="mt-4 border-t pt-4 text-[13.5px]">
            <Link to="/connect" className="text-link hover:underline">
              Snippets for each SDK, and the environment block
            </Link>
          </div>
        </Card>
      </div>

      <Signals live={h?.signals} checks={checks.data} checksError={checks.error} />

      <Section
        title="Recent activity"
        testId="recent"
        lede={h ? `The newest calls from apps. Live holds ${number(h.ring.held)} of ${number(h.ring.capacity)}.` : undefined}
        actions={
          <Button asChild variant="outline" className="bg-card">
            <Link to="/live" data-testid="home-open-live">
              <RadioIcon /> Open Live
            </Link>
          </Button>
        }
      >
        {h && h.recent.length === 0 ? (
          <div className="flex items-center gap-3 rounded-xl border border-dashed bg-card px-5 py-6 text-[13.5px] text-muted-foreground" data-testid="recent-empty">
            <CheckCircle2Icon className="size-4" aria-hidden /> No app has called yet.
          </div>
        ) : (
          <DataTable aria-label="Recent calls" data={h?.recent ?? []} columns={columns} rowId={(r) => String(r.seq)} maxHeight={360} onRowClick={(r) => void navigate({ to: "/live", search: { call: r.seq } })} empty="Loading…" />
        )}
      </Section>

      <BuildServices called={h?.services ?? []} now={now} />
    </>
  );
}
