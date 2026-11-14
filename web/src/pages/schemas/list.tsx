// The Schemas list: the registry beside the broker, read in this process. The subjects as a DataTable (search, type and the
// soft-deleted in the address), the figures as stat tiles, where a client points, and, when there is no registry, why
// (Sc-14) with the command that starts one.
import { getRouteApi, Link } from "@tanstack/react-router";
import { FileJson2Icon, PlusIcon, PowerOffIcon, SearchIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useSchemas } from "@/api/schemas";
import type { SchemasResponse } from "@/api/types/SchemasResponse";
import type { SubjectRow } from "@/api/types/SubjectRow";
import { CodeValue } from "@/components/app/code-value";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { MultiSelect } from "@/components/app/multi-select";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { StatStrip } from "@/components/app/stat";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { number } from "@/lib/format";
import { PageError } from "../page-error";
import { useMinWidth } from "../r4/bits";
import { KindBadge, LevelWord, utcDay } from "./parts";
import { KINDS, KIND_WORD, type Kind } from "./search";

const route = getRouteApi("/schemas");

function columns(wide: boolean): DataColumn<SubjectRow>[] {
  const all: DataColumn<SubjectRow>[] = [
    {
      id: "subject",
      header: "Subject",
      accessorFn: (r) => r.subject,
      cell: ({ row: { original: r } }) => {
        const used = r.topics.map(([t, p]) => `${p} of ${t}`).join(", ");
        const sub = r.deleted ? "soft-deleted: not listed or looked up by clients" : used || "names no topic";
        return (
          <div className="flex min-w-0 flex-col">
            <Link to="/schemas/subjects/$subject" params={{ subject: r.subject }} className="truncate font-mono text-link hover:underline" title={r.subject} data-testid="subject-link">
              {r.subject}
            </Link>
            <span className={r.deleted ? "truncate text-[12px] text-warn" : "truncate text-[12px] text-muted-foreground"} title={sub}>
              {sub}
            </span>
          </div>
        );
      },
    },
    { id: "type", header: "Type", size: 116, accessorFn: (r) => r.kind, cell: ({ row: { original: r } }) => <KindBadge kind={r.kind} /> },
    {
      id: "latest",
      header: "Latest",
      size: 124,
      accessorFn: (r) => r.latest_version,
      meta: { mono: true },
      cell: ({ row: { original: r } }) => (
        <span className="tabular-nums">
          v{r.latest_version} <span className="text-muted-foreground">· id {r.latest_id}</span>
        </span>
      ),
    },
    { id: "versions", header: "Versions", size: 112, accessorFn: (r) => r.versions, meta: { numeric: true, mono: true } },
    {
      id: "level",
      header: "Compatibility",
      size: 188,
      accessorFn: (r) => r.level,
      cell: ({ row: { original: r } }) => <LevelWord level={r.level} own={r.level_is_own} />,
    },
    { id: "refs", header: "Refs", size: 84, accessorFn: (r) => r.references, meta: { numeric: true, mono: true } },
    { id: "registered", header: "Registered", size: 168, accessorFn: (r) => r.last_registered ?? "", meta: { mono: true }, cell: ({ row: { original: r } }) => (r.last_registered ? utcDay(r.last_registered) : <span className="text-muted-foreground">not kept</span>) },
  ];
  return wide ? all : all.filter((c) => c.id !== "refs" && c.id !== "registered");
}

/** No registry: what happened and how to start one. */
function Off({ r }: { r: SchemasResponse }) {
  const off = r.off!;
  return (
    <EmptyState icon={PowerOffIcon} title={off.title} className="py-12" action={<CodeValue value={off.command} label="Command" className="w-[min(36rem,80vw)]" />}>
      <span data-testid="schemas-off-reason">{off.reason}</span>
    </EmptyState>
  );
}

function Empty({ r }: { r: SchemasResponse }) {
  const topic = r.example_topic ?? "orders";
  const props = `bootstrap.servers=${r.listen}\nschema.registry.url=${r.url}\nvalue.serializer=io.confluent.kafka.serializers.KafkaAvroSerializer\n# auto.register.schemas=true is the default`;
  return (
    <EmptyState
      icon={FileJson2Icon}
      title="Nothing is registered yet"
      action={
        <Button asChild>
          <Link to="/schemas/new" search={{ subject: `${topic}-value` }}>
            <PlusIcon /> Register a schema
          </Link>
        </Button>
      }
    >
      This is not an error. A serializer registers its schema the first time it sends (as <code className="font-mono">{topic}-value</code> for the topic {topic}), or register one here.
      <pre className="mt-4 overflow-x-auto rounded-lg border bg-well p-3 text-left font-mono text-[12.5px] text-bright">{props}</pre>
    </EmptyState>
  );
}

export function SchemasPage() {
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const res = useSchemas();
  const q = search.q ?? "";
  const kinds = useMemo(() => search.type ?? [], [search.type]);
  const [text, setText] = useState(q);
  useEffect(() => {
    if (text.trim() === q) return;
    const t = setTimeout(() => void navigate({ search: (s) => ({ ...s, q: text.trim() || undefined }), replace: true, resetScroll: false }), 200);
    return () => clearTimeout(t);
  }, [text, q, navigate]);
  const wide = useMinWidth(1280);
  const cols = useMemo(() => columns(wide), [wide]);
  const data = res.data;
  const rows = useMemo(() => {
    const want = q.toLowerCase();
    return (data?.subjects ?? []).filter((r) => (search.deleted ? true : !r.deleted) && (kinds.length === 0 || kinds.includes(r.kind as Kind)) && (!want || r.subject.toLowerCase().includes(want)));
  }, [data, q, kinds, search.deleted]);
  const refresh = <RefreshControl updatedAt={res.dataUpdatedAt} fetching={res.isFetching} onRefresh={() => void res.refetch()} />;
  const register = data && !data.off && (
    <Button asChild data-testid="register-schema">
      <Link to="/schemas/new">
        <PlusIcon /> Register a schema
      </Link>
    </Button>
  );
  const header = <PageHeader title="Schemas" crumbs={[{ label: "Home", to: "/" }]} sub="The schema registry beside this broker: subjects, their versions and the rules between them, as Confluent's registry keeps them." refresh={refresh} actions={register} />;
  if (res.isError && !data) return <PageError error={res.error} onRetry={() => void res.refetch()} />;
  if (!data) {
    return (
      <>
        {header}
        <div className="flex flex-col gap-4" aria-busy>
          <Skeleton className="h-[106px] rounded-xl" />
          <Skeleton className="h-80 rounded-xl" />
        </div>
      </>
    );
  }
  if (data.off) {
    return (
      <>
        {header}
        <Off r={data} />
      </>
    );
  }
  const live = data.subjects.filter((s) => !s.deleted);
  const soft = data.subjects.length - live.length;
  const versions = live.reduce((a, s) => a + s.versions, 0);
  const options = KINDS.filter((k) => data.kinds.includes(k)).map((k) => ({ value: k, label: KIND_WORD[k]!, hint: number(live.filter((s) => s.kind === k).length) }));
  return (
    <>
      {header}
      <StatStrip
        stats={[
          { label: "Subjects", value: number(live.length), ch: 5, unit: soft ? `${soft} soft-deleted` : undefined },
          { label: "Versions", value: number(versions), ch: 5 },
          { label: "Schema ids", value: number(data.ids), ch: 5, unit: `next ${data.next_id}` },
          { label: "Global level", value: data.level, mono: true, ch: 13 },
          { label: "Mode", value: data.mode, mono: true, ch: 9 },
        ]}
      />
      {data.subjects.length === 0 ? (
        <div className="mt-6">
          <Empty r={data} />
        </div>
      ) : (
        <>
          <div className="mt-6 mb-3 flex flex-wrap items-center gap-3" data-testid="schemas-toolbar">
            <div className="relative w-72 max-w-full">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden />
              <Input value={text} onChange={(e) => setText(e.target.value)} placeholder="Filter subjects" aria-label="Filter subjects" className="bg-card pl-9" data-testid="schemas-filter" />
            </div>
            <MultiSelect options={options} value={kinds} onChange={(v) => void navigate({ search: (s) => ({ ...s, type: v.length ? (KINDS.filter((k) => v.includes(k)) as Kind[]) : undefined }), replace: true, resetScroll: false })} noun="types" aria-label="Types" triggerClassName="w-48" />
            <div className="flex items-center gap-2">
              <Switch id="show-deleted" checked={!!search.deleted} onCheckedChange={(on) => void navigate({ search: (s) => ({ ...s, deleted: on ? 1 : undefined }), replace: true, resetScroll: false })} data-testid="show-deleted" />
              <Label htmlFor="show-deleted" className="text-[13.5px] font-normal">
                Show soft-deleted
              </Label>
            </div>
            <span className="ml-auto text-[13px] text-muted-foreground tabular-nums" data-testid="schemas-shown">
              {number(rows.length)} of {number(search.deleted ? data.subjects.length : live.length)} subjects
            </span>
          </div>
          <DataTable data={rows} columns={cols} rowId={(r) => r.subject} aria-label="Subjects" maxHeight={680} rowHeight={60} empty={q ? `No subject contains ${q}.` : "No subject is of these types."} />
        </>
      )}
      <section className="mt-8 grid gap-4 lg:grid-cols-2" aria-label="Point a client here">
        <div className="rounded-xl border bg-card p-5">
          <h2 className="text-[15px] font-semibold text-bright">Point a client here</h2>
          <p className="mt-1 mb-3 text-[13.5px] text-muted-foreground">{data.basic_auth ? "SASL is on, so clients also send basic.auth.user.info with a broker user and its password." : "No authentication is needed."}</p>
          <CodeValue value={`schema.registry.url=${data.url}`} label="Property" />
        </div>
        <div className="rounded-xl border bg-card p-5">
          <h2 className="text-[15px] font-semibold text-bright">Check before you register</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">
            Registering checks a schema against the subject's level first, as Confluent does: a schema that would break a reader is refused with 409 and nothing is stored. The register page runs the same check as you type.
          </p>
        </div>
      </section>
    </>
  );
}
