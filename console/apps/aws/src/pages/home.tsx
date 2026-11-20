import { Link } from "@tanstack/react-router";
import { RadioIcon } from "lucide-react";
import { useCallback, useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { clockUtc, dateTimeUtc, number } from "@doze/ui/lib/format";
import { useAbout, useServices } from "../api/queries";
import type { About } from "../api/types/About";
import type { ServiceRow } from "../api/types/ServiceRow";
import { authWords, iamWords } from "../words";

const HINT = "Check that doze-aws is still running, then try again. Nothing was changed.";

function Card({ title, lede, children, testId }: { title: string; lede?: React.ReactNode; children: React.ReactNode; testId: string }) {
  return (
    <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-labelledby={`${testId}-title`} data-testid={testId}>
      <h2 id={`${testId}-title`} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
        {title}
      </h2>
      {lede && <p className="mt-1.5 text-[13.5px] text-muted-foreground">{lede}</p>}
      <div className="mt-5">{children}</div>
    </section>
  );
}

/** A value to copy: what it is, the value on one line (cut with an ellipsis, whole on hover), and Copy. */
function CopyRow({ label, value, testId }: { label: string; value: string; testId: string }) {
  return (
    <div className="flex items-center gap-4 border-t py-4 last:pb-0">
      <div className="min-w-0 flex-1">
        <div className="text-[13px] text-muted-foreground">{label}</div>
        <div className="mt-1 truncate font-mono text-[14.5px] text-bright" title={value} data-testid={testId}>
          {value}
        </div>
      </div>
      <CopyButton text={value} what={`${label} copied`} className="bg-card" />
    </div>
  );
}

function Fact({ label, children, note, testId }: { label: string; children: React.ReactNode; note?: string; testId: string }) {
  return (
    <>
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={testId}>
        {children}
        {note && <span className="mt-0.5 block text-[12.5px] text-muted-foreground">{note}</span>}
      </dd>
    </>
  );
}

function Facts({ about }: { about: About }) {
  const mono = "font-mono text-[13px]";
  return (
    <dl className="grid grid-cols-[minmax(7.5rem,10.5rem)_minmax(0,1fr)] [&>dd:last-of-type]:border-b-0 [&>dd:last-of-type]:pb-0 [&>dt:last-of-type]:border-b-0 [&>dt:last-of-type]:pb-0">
      <Fact label="Version" testId="fact-version">
        <span className={mono}>{about.version}</span>
      </Fact>
      <Fact label="Account" testId="fact-account" note="Every request is for this account.">
        <span className={mono}>{about.account}</span>
      </Fact>
      <Fact label="Region" testId="fact-region" note={about.regions.length > 1 ? `Calls in the Live record were signed for: ${about.regions.join(", ")}.` : "The default (--region), and the only one calls in the Live record were signed for."}>
        <span className={mono}>{about.region}</span>
      </Fact>
      <Fact label="Signatures" testId="fact-auth" note={authWords(about.auth)}>
        <span className={mono}>{about.auth}</span>
      </Fact>
      <Fact label="IAM" testId="fact-iam" note={iamWords(about.iam).sentence}>
        <span className={mono}>{about.iam}</span>
      </Fact>
      <Fact label="Data directory" testId="fact-data-dir" note="Where state is kept on this machine.">
        <span className={`${mono} break-all`}>{about.data_dir}</span>
      </Fact>
      <Fact label="Console" testId="fact-console" note="This page's own address. It has no login and answers this machine only.">
        <span className={`${mono} break-all`}>{about.console}</span>
      </Fact>
      <Fact label="Process" testId="fact-process" note={`Started ${dateTimeUtc(about.started * 1000)}.`}>
        <span className={mono}>pid {about.pid}</span>
      </Fact>
    </dl>
  );
}

function CardSkeleton({ rows }: { rows: number }) {
  return (
    <div aria-busy aria-label="Loading">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-6 border-t py-4 first:border-t-0">
          <Skeleton className="h-3.5 w-24" />
          <Skeleton className="h-3.5 w-48" />
        </div>
      ))}
    </div>
  );
}

const percent = (r: ServiceRow) => (r.total > 0 ? Math.round((r.built / r.total) * 100) : 0);

const SERVICE_COLUMNS: DataColumn<ServiceRow>[] = [
  { id: "title", header: "Service", accessorKey: "title", cell: (c) => <span className="font-medium text-bright">{c.getValue<string>()}</span> },
  { id: "name", header: "Name", size: 170, accessorKey: "name", meta: { mono: true } },
  {
    id: "state",
    header: "This run",
    size: 150,
    accessorFn: (r) => (r.served ? "Served" : "Not served"),
    cell: (c) => <StatusDot tone={c.row.original.served ? "ok" : "idle"}>{c.getValue<string>()}</StatusDot>,
  },
  { id: "built", header: "Operations built", size: 190, accessorKey: "built", sortDescFirst: true, meta: { numeric: true }, cell: (c) => `${number(c.row.original.built)} of ${number(c.row.original.total)}` },
  { id: "share", header: "Share", size: 100, accessorFn: percent, sortDescFirst: true, meta: { numeric: true }, cell: (c) => `${c.getValue<number>()}%` },
  { id: "apis", header: "APIs", size: 260, enableSorting: false, accessorFn: (r) => r.apis.map((a) => a.sdk_id).join(", ") },
];

/** Home: what this doze-aws is and where an app connects (`/api/v1/about`), and the services of the build with how much of
 * each is built (`/api/v1/services`). Nothing here is not said by one of the two. */
export function HomePage() {
  const about = useAbout();
  const services = useServices();
  const { refetch: refetchAbout } = about;
  const { refetch: refetchServices } = services;
  const refresh = useCallback(() => {
    void refetchAbout();
    void refetchServices();
  }, [refetchAbout, refetchServices]);
  // A refresh that failed while the page still shows what an earlier one brought: say so, and from when.
  const stale = about.isError && about.data ? about : services.isError && services.data ? services : null;
  const rows = services.data?.services;
  const counts = useMemo(() => (rows ? { served: rows.filter((r) => r.served).length, all: rows.length, built: rows.reduce((n, r) => n + r.built, 0), total: rows.reduce((n, r) => n + r.total, 0) } : null), [rows]);
  const a = about.data;
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
        refresh={<RefreshControl updatedAt={Math.max(about.dataUpdatedAt, services.dataUpdatedAt)} fetching={about.isFetching || services.isFetching} onRefresh={refresh} />}
        actions={
          <Button asChild variant="outline" className="bg-card">
            <Link to="/live" data-testid="home-open-live">
              <RadioIcon /> Open Live
            </Link>
          </Button>
        }
      />

      {stale && (
        <ErrorBlock className="mb-8" what="This page could not be refreshed" error={stale.error} hint={`What is shown is from ${clockUtc(stale.dataUpdatedAt)} UTC. ${HINT}`} onRetry={refresh} retrying={about.isFetching || services.isFetching} testId="stale-error" />
      )}

      {about.isError && !a ? (
        <ErrorBlock className="mb-8" what="What this doze-aws is could not be loaded" error={about.error} hint={HINT} onRetry={() => void about.refetch()} retrying={about.isFetching} testId="about-error" />
      ) : (
        <div className="mb-10 grid items-start gap-6 xl:grid-cols-2">
          <Card testId="card-connect" title="Where your app connects" lede="Point an AWS SDK or the AWS CLI at this address. Any region works; signatures are checked only when doze-aws is started with --auth verify.">
            {a ? (
              <>
                <CopyRow label="Endpoint" value={a.endpoint} testId="endpoint" />
                <CopyRow label="As an environment variable" value={`AWS_ENDPOINT_URL=${a.endpoint}`} testId="endpoint-env" />
              </>
            ) : (
              <CardSkeleton rows={2} />
            )}
          </Card>
          <Card testId="card-about" title="This doze-aws">
            {a ? <Facts about={a} /> : <CardSkeleton rows={6} />}
          </Card>
        </div>
      )}

      <section aria-labelledby="services-title" data-testid="services">
        <h2 id="services-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
          Services
        </h2>
        <p className="mt-1.5 mb-5 max-w-[46rem] text-[13.5px] text-muted-foreground">
          {counts ? (
            <span data-testid="services-counts">
              {number(counts.served)} of {number(counts.all)} are served in this run; {number(counts.built)} of {number(counts.total)} operations are built. A service left out of --services answers every call with how to turn it on.
            </span>
          ) : (
            "Every service of this build, and how many of AWS's operations each answers."
          )}
        </p>
        {services.isError && !rows ? (
          <ErrorBlock what="The services could not be loaded" error={services.error} hint={HINT} onRetry={() => void services.refetch()} retrying={services.isFetching} testId="services-error" />
        ) : rows ? (
          <DataTable aria-label="Services of this build" data={rows} columns={SERVICE_COLUMNS} rowId={(r) => r.name} maxHeight={720} empty="This build has no services." />
        ) : (
          <div className="rounded-xl border bg-card px-6 py-2">
            <CardSkeleton rows={6} />
          </div>
        )}
      </section>
    </>
  );
}
