import { Link } from "@tanstack/react-router";
import { ActivityIcon, ArrowRightIcon, CircleCheckIcon, CodeIcon, InfoIcon, PlusIcon, SettingsIcon, TriangleAlertIcon } from "lucide-react";
import { useHome } from "@/api/queries";
import type { HomeResponse } from "@/api/types/HomeResponse";
import { CodeValue } from "@/components/app/code-value";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { Spans } from "@/components/app/spans";
import { type Stat, StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { clockUtc, duration, hhmmUtc, number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { CheckAgain, ProbeRules, VerdictDot } from "./probe";
import { PageError } from "./page-error";

function stats(r: HomeResponse, now: number): Stat[] {
  const h = r.home;
  const i = h.instance;
  const stop = i.last_stop === "clean" ? "last stop clean" : i.last_stop === "unclean" ? "last stop not clean" : "on a new data directory";
  const spoke = h.spoke;
  return [
    { label: "Started", value: i.started.replace(/ UTC$/, ""), unit: stop, mono: true, ch: 12, tip: `Up ${duration(i.up_secs * 1000)} when this page was fetched` },
    { label: "Topics", value: number(h.topics), unit: `${number(h.partitions)} partitions`, ch: 5 },
    { label: "Open partitions", value: r.words.open_partitions, unit: "opened on first use", mono: true, ch: 8 },
    spoke
      ? { label: "Apps on the wire", value: number(spoke.apps), unit: spoke.oldest_ms ? `back to ${hhmmUtc(spoke.oldest_ms)} UTC` : "no request yet", ch: 5 }
      : { label: "Apps on the wire", value: "–", unit: "no recorder", ch: 5 },
    spoke?.last_ms
      ? { label: "Last request", value: clockUtc(spoke.last_ms), unit: spoke.last_client ? `from ${spoke.last_client}` : "UTC", mono: true, ch: 9, tip: `${duration(now - spoke.last_ms)} before this page was fetched` }
      : { label: "Last request", value: "none yet", mono: true, ch: 9 },
  ];
}

function Callout({ r }: { r: HomeResponse }) {
  const s = r.home.state;
  if (s.kind === "attention") {
    const pct = s.timeout_secs > 0 ? Math.min(100, Math.max(0, (s.elapsed_secs * 100) / s.timeout_secs)) : 0;
    return (
      <div className="flex gap-3 rounded-xl border border-warn/30 bg-warn/5 p-5" role="status" data-testid="home-callout" data-state="attention">
        <TriangleAlertIcon className="mt-0.5 size-5 shrink-0 text-warn" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0 flex-1 basis-80 text-[14px]">
              <b className="font-semibold text-bright">{r.words.attention_headline}</b>
              <p className="mt-1 text-muted-foreground">
                {r.words.unchecked_note && `${r.words.unchecked_note} `}Transaction <code className="font-mono text-foreground">{s.transactional_id}</code> (producer {s.producer_id}, epoch {s.producer_epoch}) is open on{" "}
                <code className="font-mono text-foreground">{s.partition}</code>. If its app does not commit or abort before the timeout, the coordinator aborts it and the producer is fenced.
              </p>
            </div>
          </div>
          <div className="mt-4 grid max-w-xl grid-cols-[1fr_auto] items-center gap-3">
            <div className="h-1.5 overflow-hidden rounded-full bg-track" aria-hidden>
              <div className="h-full rounded-full bg-warn" style={{ width: `${pct}%` }} />
            </div>
            <span className="text-[12.5px] whitespace-nowrap text-muted-foreground tabular-nums">
              open since {s.since} · timeout {s.timeout_secs} s
            </span>
          </div>
        </div>
      </div>
    );
  }
  if (s.kind === "quiet") {
    return (
      <div className="flex items-start gap-3 rounded-xl border bg-card p-5" role="status" data-testid="home-callout" data-state="quiet">
        <InfoIcon className="mt-0.5 size-5 shrink-0 text-cool" aria-hidden />
        <p className="text-[14px] text-muted-foreground">
          <b className="font-semibold text-bright">Nothing to look after yet.</b> This broker has no topics. The next step is to send it something.
        </p>
      </div>
    );
  }
  return (
    <div className="flex items-start gap-3 rounded-xl border bg-card p-5" role="status" data-testid="home-callout" data-state="steady">
      {r.words.unchecked_note ? <InfoIcon className="mt-0.5 size-5 shrink-0 text-cool" aria-hidden /> : <CircleCheckIcon className="mt-0.5 size-5 shrink-0 text-good" aria-hidden />}
      <p className="text-[14px] text-muted-foreground">
        {r.words.unchecked_note ? (
          <>
            <b className="font-semibold text-bright">No transaction that was checked is near its timeout.</b> {r.words.unchecked_note} This is not the all-clear.
          </>
        ) : (
          <>
            <b className="font-semibold text-bright">Nothing needs you.</b> No transaction is near its timeout. When something does, it shows here.
          </>
        )}
      </p>
    </div>
  );
}

function AddressCard({ r }: { r: HomeResponse }) {
  const h = r.home;
  return (
    <Card className="min-w-0" data-testid="address-card">
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
        <CardTitle className="text-[17px]">Address</CardTitle>
        <Link to="/connect" className="text-[13px] font-medium text-primary hover:underline">
          Snippets for every client
        </Link>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <p className="text-[13.5px] text-muted-foreground">Point any Kafka client at this address. It is where the broker listens and, below, where it tells clients to reconnect.</p>
        <CodeValue value={h.listen} label="Address" className="h-11 text-[15px]" />
        <p className="text-[12.5px] text-muted-foreground">
          The listener, set by <code className="font-mono">--listen</code>. {h.loopback ? "On loopback it is reachable from this machine only." : "It is not on loopback, so other machines can reach it."}
        </p>
        <Separator className="my-3" />
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-3">
            <h3 className="text-[15px] font-semibold text-bright">Advertised listener</h3>
            <VerdictDot verdict={h.probe.verdict} />
          </div>
          <CheckAgain />
        </div>
        <p className="text-[13px] text-muted-foreground">{r.words.advertised_lede}</p>
        <ProbeRules probe={h.probe} />
      </CardContent>
    </Card>
  );
}

function InstanceCard({ r }: { r: HomeResponse }) {
  const i = r.home.instance;
  const rows: [string, React.ReactNode][] = [
    ["Started", <span className="font-mono">{i.started}</span>],
    ["Last stop", i.last_stop === "clean" ? <StatusDot tone="ok">Clean</StatusDot> : i.last_stop === "unclean" ? <StatusDot tone="warn">Not clean</StatusDot> : "None yet"],
    ["Open partitions", <span className="font-mono">{r.words.open_partitions}</span>],
  ];
  if (i.open_files !== null) rows.push(["Descriptors", <span className="font-mono">{number(i.open_files)}</span>]);
  rows.push(["Version profile", i.profile]);
  rows.push([
    "Access",
    <span className="flex flex-col">
      <Link to="/access" className="font-medium text-primary hover:underline">
        {i.access.label}
      </Link>
      <span className="text-[12.5px] text-muted-foreground">{i.access.detail}</span>
    </span>,
  ]);
  return (
    <Card className="min-w-0" data-testid="instance-card">
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <CardTitle className="text-[17px]">This instance</CardTitle>
        <StatusDot tone="ok">Running</StatusDot>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <p className="text-[13.5px] text-muted-foreground">{r.words.instance_lede}</p>
        <dl className="grid grid-cols-[minmax(7.5rem,10rem)_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-[13.5px]">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className="min-w-0 text-bright">{v}</dd>
            </div>
          ))}
        </dl>
        {i.data_dir && (
          <div className="flex flex-col gap-1.5">
            <span className="text-[12.5px] text-muted-foreground">Data directory</span>
            <CodeValue value={i.data_dir} label="Data directory" />
          </div>
        )}
        <div>
          <Button variant="outline" asChild className="bg-card">
            <Link to="/settings">
              <SettingsIcon /> Weight and data in Settings
            </Link>
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function NextCard({ r }: { r: HomeResponse }) {
  const h = r.home;
  if (h.state.kind === "quiet") {
    const steps: [string, string, React.ReactNode][] = [
      ["1", "Ask the broker what it has", <CodeValue value={`kcat -b ${h.listen} -L`} label="Command" />],
      ["2", "Send a message (auto-create makes the topic, 1 partition)", <CodeValue value={`echo "hello" | kcat -P -b ${h.listen} -t orders -X client.id=admin-cli`} label="Command" />],
      ["3", "Watch it arrive", <p className="text-[13px] text-muted-foreground">Open Live: the request is the first row on the wire, with its caller, topic and result.</p>],
    ];
    return (
      <Card data-testid="next-card">
        <CardHeader className="flex flex-row items-center justify-between gap-2">
          <CardTitle className="text-[17px]">Waiting for your first request</CardTitle>
          <StatusDot tone="idle">Listening</StatusDot>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <p className="text-[13.5px] text-muted-foreground">Three steps get a client through, each a command you can paste.</p>
          <ol className="flex flex-col gap-4">
            {steps.map(([n, t, body]) => (
              <li key={n} className="flex gap-3">
                <span className="flex size-6 shrink-0 items-center justify-center rounded-full border bg-well font-mono text-[12px]">{n}</span>
                <div className="flex min-w-0 flex-1 flex-col gap-2">
                  <span className="text-[13.5px] font-medium text-bright">{t}</span>
                  {body}
                </div>
              </li>
            ))}
          </ol>
          <div className="flex flex-wrap gap-2">
            <Button asChild>
              <Link to="/connect">
                <CodeIcon /> Snippets for your client
              </Link>
            </Button>
            <Button variant="outline" asChild className="bg-card">
              <Link to="/topics">
                <PlusIcon /> Create a topic first
              </Link>
            </Button>
          </div>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card data-testid="next-card">
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <CardTitle className="text-[17px]">Next: connect something new</CardTitle>
        <StatusDot tone="ok">Listening</StatusDot>
      </CardHeader>
      <CardContent className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] xl:items-end">
        <p className="text-[13.5px] text-muted-foreground">Point a client at the address and name it with client.id, so that it appears as itself on the wire. When a client id this broker has not seen sends its first request, it shows in Live with the versions it speaks.</p>
        <CodeValue value={r.words.first_command} label="Command" />
        <div className="flex flex-wrap gap-2 xl:col-span-2">
          <Button asChild>
            <Link to="/connect">
              <CodeIcon /> Connect an app
            </Link>
          </Button>
          <Button variant="outline" asChild className="bg-card">
            <Link to="/live">
              <ActivityIcon /> Open Live
            </Link>
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function HomeSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-busy>
      <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
        {Array.from({ length: 5 }, (_, i) => (
          <Skeleton key={i} className="h-[98px] rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-16 rounded-xl" />
      <div className="grid gap-6 xl:grid-cols-[minmax(0,7fr)_minmax(380px,5fr)]">
        <Skeleton className="h-96 rounded-xl" />
        <Skeleton className="h-96 rounded-xl" />
      </div>
    </div>
  );
}

export function HomePage() {
  const q = useHome();
  const r = q.data;
  const quiet = r?.home.state.kind === "quiet";
  return (
    <>
      <PageHeader
        title={r?.words.title ?? "Home"}
        sub={r ? <Spans spans={r.words.sub} /> : " "}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} />}
        actions={
          r && !quiet ? (
            <Button asChild>
              <Link to="/connect">
                <CodeIcon /> Connect an app <ArrowRightIcon className="opacity-70" />
              </Link>
            </Button>
          ) : undefined
        }
      />
      {q.isError && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !r ? (
        <HomeSkeleton />
      ) : (
        <div className="flex flex-col gap-6" data-testid="home">
          <StatStrip stats={stats(r, q.dataUpdatedAt)} />
          <Callout r={r} />
          <div className={cn("grid gap-6 xl:grid-cols-[minmax(0,7fr)_minmax(380px,5fr)] xl:items-start")}>
            <AddressCard r={r} />
            <InstanceCard r={r} />
          </div>
          <NextCard r={r} />
        </div>
      )}
    </>
  );
}
