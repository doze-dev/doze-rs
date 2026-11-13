// The page a new client lands on (board connect-first-request; classic pages/live/first.rs): who it is, what it asked in
// order, and what to do next. A snapshot with the RefreshControl.
import { Link } from "@tanstack/react-router";
import { ActivityIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon, UserRoundSearchIcon } from "lucide-react";
import { useFirst } from "@/api/live";
import type { FirstView } from "@/api/types/FirstView";
import { CodeValue } from "@/components/app/code-value";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { PageError } from "../page-error";
import { Tape } from "./tape";
import { clockMs } from "./words";

const CRUMBS = [
  { label: "Home", to: "/" },
  { label: "Connect", to: "/connect" },
];

function Banner({ v }: { v: FirstView }) {
  return v.all_answered ? (
    <div className="flex items-start gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[14px]" data-testid="first-banner" data-ok="1">
      <CircleCheckIcon className="mt-0.5 size-5 shrink-0 text-good" aria-hidden />
      <p className="text-muted-foreground">
        <b className="font-semibold text-bright">It works.</b> The client reached the listener, negotiated versions and was answered. Nothing it asked for was refused.
      </p>
    </div>
  ) : (
    <div className="flex items-start gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4 text-[14px]" data-testid="first-banner" data-ok="0">
      <TriangleAlertIcon className="mt-0.5 size-5 shrink-0 text-warn" aria-hidden />
      <p className="text-muted-foreground">
        <b className="font-semibold text-bright">It reached the broker.</b> {v.refused} of its requests {v.refused === 1 ? "was" : "were"} refused or not answered: open the row for Kafka's words.
      </p>
    </div>
  );
}

function Who({ v }: { v: FirstView }) {
  const rows: [string, React.ReactNode][] = [
    ["client.id", <span className="font-mono">{v.client}</span>],
    [
      "Software",
      <>
        {v.software?.trim() || "not announced"} <span className="text-muted-foreground">(from ApiVersions)</span>
      </>,
    ],
    ["From", <span className="font-mono">{v.peer ?? "-"}</span>],
    ["First request", <span className="font-mono">{clockMs(v.first_ms)} UTC</span>],
    [
      "Version range",
      <>
        <span className="font-mono text-[12.5px]">{v.versions.map(([a, n]) => `${a} v${n}`).join(", ")}</span>
        <span className="mt-0.5 block text-[12.5px] text-muted-foreground">Each API is spoken at the highest version both sides know.</span>
      </>,
    ],
  ];
  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle className="text-[17px]">Who it is</CardTitle>
      </CardHeader>
      <CardContent>
        <dl className="grid grid-cols-[minmax(7rem,9rem)_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-[13.5px]">
          {rows.map(([k, val]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className="min-w-0 break-words text-bright">{val}</dd>
            </div>
          ))}
        </dl>
      </CardContent>
    </Card>
  );
}

function Story({ v }: { v: FirstView }) {
  return (
    <Card className="min-w-0">
      <CardHeader>
        <CardTitle className="text-[17px]">What it asked, in order</CardTitle>
      </CardHeader>
      <CardContent>
        <ol className="flex flex-col border-l pl-4" data-testid="first-story">
          {v.rows.map((r) => (
            <li key={r.id} className="relative py-1.5 text-[13.5px]">
              <span aria-hidden className="absolute top-3 -left-[21px] size-2 rounded-full bg-primary" />
              <span className="mr-3 font-mono text-[12px] text-muted-foreground">{clockMs(r.at_ms)}</span>
              <b className="font-semibold text-bright">
                {r.api} v{r.version}
              </b>
              <span className="block truncate pl-[7.5rem] text-muted-foreground" title={r.detail ? `${r.target} · ${r.detail}` : r.target}>
                {r.target}
                {r.detail && ` · ${r.detail}`}
              </span>
            </li>
          ))}
          <li className="relative py-1.5 text-[13.5px] text-muted-foreground">
            <span aria-hidden className="absolute top-3 -left-[21px] size-2 rounded-full border bg-card" />
            <span className="mr-3 font-mono text-[12px]">next</span>
            Waiting for its next request: a produce, a fetch or a group call.
          </li>
        </ol>
      </CardContent>
    </Card>
  );
}

export function FirstRequestPage() {
  const q = useFirst();
  const v = q.data?.first;
  return (
    <>
      <PageHeader
        crumbs={CRUMBS}
        title="A new client just connected"
        sub={
          v ? (
            <>
              <b className="font-mono font-medium text-bright">{v.client}</b> sent its first request{v.peer ? `, from ${v.peer}` : ""}.{v.software?.trim() && <> It speaks Kafka as <span className="font-mono">{v.software.trim()}</span>.</>}
            </>
          ) : (
            " "
          )
        }
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} />}
        actions={
          <Button asChild>
            <Link to="/live">
              <ActivityIcon /> Open Live
            </Link>
          </Button>
        }
      />
      {q.isError && !q.data ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !q.data ? (
        <Skeleton className="h-96 rounded-xl" />
      ) : !v ? (
        <EmptyState
          icon={UserRoundSearchIcon}
          title="No client has connected yet"
          action={
            <Button asChild variant="outline" className="bg-card">
              <Link to="/connect">Snippets for your client</Link>
            </Button>
          }
        >
          When one sends its first request it appears here, with the versions it speaks.
        </EmptyState>
      ) : (
        <div className="flex flex-col gap-6" data-testid="first-request">
          <Banner v={v} />
          <div className="grid gap-6 xl:grid-cols-2 xl:items-start">
            <Who v={v} />
            <Story v={v} />
          </div>
          <section className="flex flex-col gap-3">
            <h2 className="text-[19px] font-medium text-bright">The wire for this client</h2>
            <p className="text-[13.5px] text-muted-foreground">
              The same rows Live shows, filtered to <span className="font-mono">{v.client}</span>.
            </p>
            <Tape rows={v.rows} layout="first" label="A client's first requests" empty="No request yet." maxHeight={480} />
            <div className="mt-2 flex items-start gap-3 rounded-xl border bg-card p-4 text-[13.5px]">
              <InfoIcon className="mt-0.5 size-[18px] shrink-0 text-cool" aria-hidden />
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <p className="text-muted-foreground">
                  <b className="font-semibold text-bright">Next:</b> send a record and watch it land, then open{" "}
                  <Link to="/topics" className="font-medium text-link hover:underline">
                    Topics
                  </Link>
                  .
                </p>
                <CodeValue value={`echo hi | kcat -P -b ${v.listen} -t orders -X client.id=${v.client}`} label="Command" />
              </div>
            </div>
          </section>
        </div>
      )}
    </>
  );
}
