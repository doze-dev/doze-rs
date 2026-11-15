// One subject: its versions, the version on screen laid out (or as stored) with Copy, its references both ways and the other
// subjects holding its id, the subject's level and mode, the topics its name points at, and its overlays (compatibility,
// delete a version or the subject, Try), each routed by `?dialog=`.
import { getRouteApi, Link } from "@tanstack/react-router";
import { FileSearchIcon, FlaskConicalIcon, GitCompareIcon, PlusIcon, SlidersHorizontalIcon, Trash2Icon } from "lucide-react";
import { ApiFailure } from "@/api/client";
import { useSubject } from "@/api/schemas";
import type { SubjectResponse } from "@/api/types/SubjectResponse";
import type { VersionShown } from "@/api/types/VersionShown";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { StatStrip } from "@/components/app/stat";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useRoutedOverlay } from "../groups/overlay";
import { PageError } from "../page-error";
import { SubjectDialogs } from "./dialogs";
import { KindBadge, SchemaBlock, utcDay } from "./parts";
import { KIND_WORD, type SubjectDialog } from "./search";

const route = getRouteApi("/schemas/subjects/$subject");

function Panel({ title, aside, children, className, testId }: { title: string; aside?: React.ReactNode; children: React.ReactNode; className?: string; testId?: string }) {
  return (
    <section className={cn("rounded-xl border bg-card p-5", className)} data-testid={testId}>
      <div className="mb-3 flex min-h-8 flex-wrap items-center justify-between gap-2">
        <h2 className="text-[15px] font-semibold text-bright">{title}</h2>
        {aside}
      </div>
      {children}
    </section>
  );
}

function Versions({ d, on }: { d: SubjectResponse; on: number }) {
  return (
    <Panel title="Versions" aside={<span className="text-[12.5px] text-muted-foreground">newest first</span>} testId="versions">
      <ol className="flex max-h-[30rem] flex-col gap-1 overflow-y-auto">
        {d.versions.map((v) => (
          <li key={v.version}>
            <Link
              to="."
              search={(s: Record<string, unknown>) => ({ ...s, version: v.version === d.row.latest_version ? undefined : v.version })}
              replace
              resetScroll={false}
              aria-current={v.version === on ? "true" : undefined}
              data-testid="version-link"
              className={cn(
                "flex items-center justify-between gap-3 rounded-lg border border-transparent px-3 py-2 text-[13.5px] hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none",
                v.version === on && "border-primary/30 bg-accent/60",
              )}
            >
              <span className="flex min-w-0 flex-col">
                <span className="font-mono text-bright">
                  v{v.version}
                  {v.version === d.row.latest_version && <span className="ml-2 font-sans text-[12px] text-link">latest</span>}
                  {v.deleted && <span className="ml-2 font-sans text-[12px] text-warn">soft-deleted</span>}
                </span>
                <span className="truncate text-[12px] text-muted-foreground">
                  {v.registered_at ? utcDay(v.registered_at) : "time not kept"} · {v.registered_by || "unknown"}
                </span>
              </span>
              <span className="font-mono text-[12px] text-muted-foreground tabular-nums">id {v.id}</span>
            </Link>
          </li>
        ))}
      </ol>
    </Panel>
  );
}

function Lines({ rows, empty }: { rows: [React.ReactNode, React.ReactNode][]; empty: string }) {
  if (rows.length === 0) return <p className="text-[13px] text-muted-foreground">{empty}</p>;
  return (
    <dl className="flex flex-col">
      {rows.map(([a, b], i) => (
        <div key={i} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 border-b py-2 text-[13px] last:border-b-0">
          <dt className="min-w-0 truncate font-mono">{a}</dt>
          <dd className="font-mono text-muted-foreground">{b}</dd>
        </div>
      ))}
    </dl>
  );
}

function Shown({ s, stored, open }: { s: VersionShown; stored: boolean; open: (dialog: SubjectDialog, from: HTMLElement) => void }) {
  const navigate = route.useNavigate();
  return (
    <Panel
      title={`Version ${s.version} · id ${s.id}`}
      testId="version-shown"
      aside={
        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup type="single" variant="outline" size="sm" value={stored ? "stored" : "laid"} onValueChange={(v) => v && void navigate({ search: (x) => ({ ...x, stored: v === "stored" ? 1 : undefined }), replace: true, resetScroll: false })} aria-label="How the schema is shown" data-testid="schema-view">
            <ToggleGroupItem value="laid" className="px-2.5 text-[12.5px]">
              Laid out
            </ToggleGroupItem>
            <ToggleGroupItem value="stored" className="px-2.5 text-[12.5px]">
              As stored
            </ToggleGroupItem>
          </ToggleGroup>
          <Button variant="outline" size="sm" className="border-bad/40 text-bad hover:bg-bad/5 hover:text-bad" onClick={(e) => open("delete-version", e.currentTarget)} data-testid="open-delete-version">
            <Trash2Icon /> {s.deleted ? "Delete for good" : "Delete version"}
          </Button>
        </div>
      }
    >
      <p className="mb-3 text-[13px] text-muted-foreground">
        {KIND_WORD[s.kind] ?? s.kind}, registered {s.registered_at ? utcDay(s.registered_at) : "at a time not kept"} by {s.registered_by || "an unknown client"}.{s.deleted && " Soft-deleted: clients cannot look it up by version, but its id still resolves."}
      </p>
      <SchemaBlock text={stored ? s.stored : s.schema} testId="schema-text" />
      <div className="mt-5 grid gap-5 xl:grid-cols-3">
        <div>
          <h3 className="mb-2 text-[13px] font-semibold text-bright">References</h3>
          <Lines
            rows={s.references.map((r) => [
              <Link key={r.name} to="/schemas/subjects/$subject" params={{ subject: r.subject }} search={{ version: r.version }} className="text-link hover:underline" title={r.name}>
                {r.name}
              </Link>,
              `${r.subject} v${r.version}`,
            ])}
            empty="It names no other schema."
          />
        </div>
        <div>
          <h3 className="mb-2 text-[13px] font-semibold text-bright">Referenced by</h3>
          <Lines
            rows={s.referenced_by.map((h) => [
              <Link key={`${h.subject}-${h.version}`} to="/schemas/subjects/$subject" params={{ subject: h.subject }} search={{ version: h.version }} className="text-link hover:underline">
                {h.subject}
              </Link>,
              `v${h.version} · id ${h.id}`,
            ])}
            empty="No schema names it."
          />
        </div>
        <div>
          <h3 className="mb-2 text-[13px] font-semibold text-bright">Same id elsewhere</h3>
          <Lines
            rows={s.same_id.map((h) => [
              <Link key={`${h.subject}-${h.version}`} to="/schemas/subjects/$subject" params={{ subject: h.subject }} search={{ version: h.version }} className="text-link hover:underline">
                {h.subject}
              </Link>,
              `v${h.version}`,
            ])}
            empty={`No other subject holds id ${s.id}.`}
          />
        </div>
      </div>
      {s.resolved && (
        <details className="mt-5">
          <summary className="cursor-pointer text-[13px] font-medium text-link">The referenced types written out</summary>
          <SchemaBlock text={s.resolved} className="mt-2" label="Copy resolved schema" />
        </details>
      )}
    </Panel>
  );
}

export function SubjectPage() {
  const { subject } = route.useParams();
  const search = route.useSearch();
  const res = useSubject(subject, search.version);
  const overlay = useRoutedOverlay("dialog");
  const open = (dialog: SubjectDialog, from: HTMLElement) => overlay.open(dialog, from);
  if (res.isError && !res.data) {
    if (res.error instanceof ApiFailure && res.error.status === 404) {
      return (
        <>
          <PageHeader title="No such subject" crumbs={[{ label: "Schemas", to: "/schemas" }]} />
          <EmptyState
            icon={FileSearchIcon}
            title={subject}
            action={
              <>
                <Button asChild variant="outline">
                  <Link to="/schemas">Back to Schemas</Link>
                </Button>
                <Button asChild>
                  <Link to="/schemas/new" search={{ subject }}>
                    Register {subject}
                  </Link>
                </Button>
              </>
            }
          >
            <span data-testid="subject-missing">{res.error.message}</span>
          </EmptyState>
        </>
      );
    }
    return <PageError error={res.error} onRetry={() => void res.refetch()} />;
  }
  const d = res.data;
  const refresh = <RefreshControl updatedAt={res.dataUpdatedAt} fetching={res.isFetching} onRefresh={() => void res.refetch()} />;
  return (
    <>
      <PageHeader
        title={subject}
        mono
        crumbs={[{ label: "Schemas", to: "/schemas" }]}
        refresh={refresh}
        sub={
          d ? (
            <span className="flex flex-wrap items-center gap-2" data-testid="subject-pills">
              <KindBadge kind={d.row.kind} />
              <Badge variant="outline" className="rounded-full bg-card font-mono font-normal text-muted-foreground">
                {d.row.level}
                {!d.row.level_is_own && " (global)"}
              </Badge>
              <Badge variant="outline" className="rounded-full bg-card font-mono font-normal text-muted-foreground">
                {d.row.mode}
              </Badge>
              {d.row.deleted && <Badge className="rounded-full bg-warn/10 font-normal text-warn">soft-deleted</Badge>}
            </span>
          ) : (
            <Skeleton className="h-6 w-72" />
          )
        }
        actions={
          <>
            <Button asChild data-testid="new-version">
              <Link to="/schemas/new" search={{ subject }}>
                <PlusIcon /> New version
              </Link>
            </Button>
            <Button asChild variant="outline" className="bg-card" data-testid="open-diff">
              <Link to="/schemas/subjects/$subject/diff" params={{ subject }}>
                <GitCompareIcon /> Compare
              </Link>
            </Button>
            <Button variant="outline" className="bg-card" onClick={(e) => open("try", e.currentTarget)} data-testid="open-try" disabled={!d || d.row.deleted}>
              <FlaskConicalIcon /> Try a value
            </Button>
            <Button variant="outline" className="border-bad/40 bg-card text-bad hover:bg-bad/5 hover:text-bad" onClick={(e) => open("delete-subject", e.currentTarget)} data-testid="open-delete-subject">
              <Trash2Icon /> Delete subject
            </Button>
          </>
        }
      />
      {d ? (
        <>
          <StatStrip
            className="mb-6"
            stats={[
              { label: "Versions", value: number(d.row.versions), ch: 4, unit: d.row.deleted_versions ? `${d.row.deleted_versions} soft-deleted` : undefined },
              { label: "Latest", value: `v${d.row.latest_version}`, mono: true, ch: 5 },
              { label: "Schema id", value: String(d.row.latest_id), mono: true, ch: 5 },
              { label: "References", value: number(d.row.references), ch: 4 },
              { label: "Topics", value: number(d.row.topics.length), ch: 4 },
            ]}
          />
          <div className="grid gap-5 lg:grid-cols-[minmax(0,20rem)_minmax(0,1fr)]">
            <div className="flex min-w-0 flex-col gap-5">
              <Versions d={d} on={d.shown.version} />
              <Panel
                title="Compatibility"
                testId="compatibility"
                aside={
                  <Button variant="outline" size="sm" onClick={(e) => open("compatibility", e.currentTarget)} data-testid="open-compatibility">
                    <SlidersHorizontalIcon /> Change
                  </Button>
                }
              >
                <dl className="flex flex-col gap-2 text-[13px]">
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted-foreground">Level</dt>
                    <dd className="font-mono" data-testid="subject-level">
                      {d.row.level}
                      {!d.row.level_is_own && <span className="ml-1.5 font-sans text-muted-foreground">(global)</span>}
                    </dd>
                  </div>
                  <div className="flex justify-between gap-3">
                    <dt className="text-muted-foreground">Mode</dt>
                    <dd className="font-mono" data-testid="subject-mode">
                      {d.row.mode}
                      {!d.row.mode_is_own && <span className="ml-1.5 font-sans text-muted-foreground">(global)</span>}
                    </dd>
                  </div>
                </dl>
                <p className="mt-3 text-[12.5px] text-muted-foreground">{d.levels.find((l) => l.name === d.row.level.replace(/_TRANSITIVE$/, ""))?.sentence}</p>
              </Panel>
              <Panel title="Used on" testId="used-on">
                {d.row.topics.length === 0 ? (
                  <p className="text-[13px] text-muted-foreground">Its name points at no topic here. A subject named for its record (RecordNameStrategy) is found by the ids on a topic's records: its Schema tab lists them.</p>
                ) : (
                  <ul className="flex flex-col gap-1.5">
                    {d.row.topics.map(([t, p]) => (
                      <li key={t} className="flex items-center justify-between gap-3 text-[13px]">
                        <Link to="/topics/$topic/schema" params={{ topic: t }} className="truncate font-mono text-link hover:underline">
                          {t}
                        </Link>
                        <span className="text-muted-foreground">{p === "record" ? "named for a record on it" : `its ${p}s`}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </Panel>
            </div>
            <div className="min-w-0">
              <Shown s={d.shown} stored={!!search.stored} open={open} />
              <p className="mt-3 truncate font-mono text-[12px] text-muted-foreground" title={`${d.url}/subjects/${subject}/versions/${d.shown.version}`}>
                GET {d.url}/subjects/{encodeURIComponent(subject)}/versions/{d.shown.version}
              </p>
            </div>
          </div>
          <SubjectDialogs subject={subject} d={d} dialog={search.dialog} onClose={overlay.close} />
        </>
      ) : (
        <div className="flex flex-col gap-4" aria-busy>
          <Skeleton className="h-[106px] rounded-xl" />
          <Skeleton className="h-96 rounded-xl" />
        </div>
      )}
    </>
  );
}
