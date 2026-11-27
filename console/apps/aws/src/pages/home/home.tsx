import { Link, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { CheckCircle2Icon, Link2Icon, RadioIcon } from "lucide-react";
import { useCallback, useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { clockMsUtc, number } from "@doze/ui/lib/format";
import { useAbout } from "../../api/queries";
import { useArrivals, useHome, useHomeChecks } from "../../api/spaces";
import type { CallRecord } from "../../api/types/CallRecord";
import type { KeyPrincipal } from "../../api/types/KeyPrincipal";
import { clientName, who } from "../../people";
import { iamWords } from "../../words";
import { Result } from "../live/outcome";
import { resourceShown } from "../live/resource";
import { Section, useNow } from "../card";
import { AwakeSection } from "./awake-section";
import { ConnectedCard, WhereCard } from "./connect-cards";
import { Signals } from "./signals";
import { shownVersion } from "./version";

const HINT = "Check that doze-aws is still running, then try again. Nothing was changed.";

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
    // The name Live shows (a queue's, not its whole URL); the value as the caller wrote it is the cell's title.
    { id: "resource", header: "Resource", accessorFn: (r) => resourceShown(r), meta: { mono: true }, cell: (c) => <span title={c.row.original.resource ?? undefined}>{c.getValue<string>()}</span> },
    { id: "result", header: "Result", size: 320, min: 260, enableSorting: false, accessorFn: (r) => r.outcome, cell: (c) => <Result call={c.row.original} /> },
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
              account <span className="font-mono text-foreground">{a.account}</span> · <span className="font-mono text-foreground">{a.region}</span> · {iamWords(a.iam).chip}{shownVersion(a.version) && <> · doze-aws <span className="font-mono text-foreground">{shownVersion(a.version)}</span></>}
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
        <WhereCard a={a} />
        <ConnectedCard home={home} now={now} endpoint={a?.endpoint} />
      </div>

      <AwakeSection activity={h?.activity} idleSecs={h?.idle_secs} now={now} />

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
    </>
  );
}
