// An event bus's page (eventbridge__bus-detail, bus-delete): what feeds it and what its rules send to, its details, its archives, and the calls
// that touched it and its rules. Delete names the rules that go with it.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Badge } from "@doze/ui/components/ui/badge";
import { Link } from "@tanstack/react-router";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, type FlowGroup, FlowStrip, RecentCalls, ResourcePage, ResourcePageSkeleton, RuleHelp, type Verb, useFeeders, useTags } from "../../../kit";
import { useArchives, useBuses, useRulesOf, useTargetsOf } from "./api";
import { BusDelete } from "./bus-delete";
import { Heading, DetailsGrid, State, when } from "./parts";
import { busCalls, resultNote, useRecord } from "./record";
import type { ArchiveItem } from "./types";
import { arnName, retentionWords, targetWords } from "./words";
import { bytes, number } from "@doze/ui/lib/format";

export function BusDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <BusView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refetch={() => void detail.refetch()} />;
}

const field = (d: ResourceDetail, label: string) => d.fields.find((f) => f.label === label)?.value ?? null;

function BusView({ d, updatedAt, fetching, refetch }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refetch: () => void }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const [deleting, setDeleting] = useState(false);
  const bus = d.name;
  const arn = field(d, "ARN") ?? "";
  const tags = useTags({ service: d.service, kind: d.kind, name: bus });
  const rules = useRulesOf(bus);
  const archives = useArchives(arn);
  const record = useRecord();
  const item = useBuses().data?.EventBuses?.find((b) => b.Name === bus);
  const ruleItems = useMemo(() => rules.data?.Rules ?? [], [rules.data]);
  const targets = useTargetsOf(bus, ruleItems);
  const feeders = useFeeders(d.calls, ["PutEvents"]);
  const calls = useMemo(() => busCalls(d.calls, record.data?.calls ?? [], ruleItems.map((r) => r.Name)), [d.calls, record.data, ruleItems]);
  const note = useMemo(() => (c: Parameters<typeof resultNote>[0]) => resultNote(c, record.data?.calls ?? []), [record.data]);
  const refresh = () => {
    refetch();
    void rules.refetch();
    void archives.refetch();
    void record.refetch();
    void tags.refetch();
  };
  const go = (to: string, s: Record<string, string>) => void navigate({ to: to as never, search: s as never });
  const onVerb = (v: Verb) => {
    if (v.id === "send") go("/resources/$service/$second", { do: "send", name: bus });
    else if (v.id === "delete") setDeleting(true);
  };
  // `payment-settled -> pick-lists queue and order-placed -> orders-created topic`: each rule with its own targets, so which rule feeds which is not lost.
  const pairs: FlowGroup[] = ruleItems.flatMap((r, i): FlowGroup[] => [
    { ...(i === 0 ? { label: "Rules" } : { before: "and" as const }), chips: [{ label: r.Name, to: { service: "eventbridge", kind: "rules", name: bus === "default" ? r.Name : `${bus}/${r.Name}` } }] },
    // A rule with no target says so, or the next pair would read as its own.
    { chips: targets[r.Name] === undefined ? [] : targets[r.Name]!.length === 0 ? [{ label: "no target" }] : [...new Set(targets[r.Name]!.map((t) => targetWords(t.Arn)))].map((l) => ({ label: l })) },
  ]);
  const groups: FlowGroup[][] = [
    ...(feeders.length > 0 ? [[{ label: "Fed by", chips: feeders.map((l) => ({ label: l })) }, { chips: [{ label: bus, self: true }] }]] : []),
    pairs,
  ];
  const kms = field(d, "KMS key");
  const dlq = field(d, "Dead-letter queue");
  const all = archives.data?.Archives ?? [];
  const columns: DataColumn<ArchiveItem>[] = [
    { id: "name", header: "Name", min: 180, accessorFn: (a) => a.ArchiveName, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "eventbridge", kind: "archives", name: c.row.original.ArchiveName }} className="font-mono text-[13px] font-medium text-link hover:underline">{c.row.original.ArchiveName}</Link> },
    { id: "state", header: "State", size: 140, accessorFn: (a) => a.State ?? "", cell: (c) => <State state={c.row.original.State} /> },
    { id: "retention", header: "Retention", size: 160, accessorFn: (a) => a.RetentionDays ?? 0, cell: (c) => retentionWords(c.row.original.RetentionDays) },
    { id: "events", header: "Events", size: 130, accessorFn: (a) => a.EventCount ?? 0, meta: { numeric: true }, cell: (c) => number(c.row.original.EventCount ?? 0) },
    { id: "size", header: "Size", size: 120, accessorFn: (a) => a.SizeBytes ?? 0, meta: { numeric: true }, cell: (c) => bytes(c.row.original.SizeBytes ?? 0) },
  ];
  const shown: ResourceDetail = { ...d, wired: [] };
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={`Created ${when(item?.CreationTime)}. Last modified ${when(item?.LastModifiedTime)}.`}
        banner={search.done === "created" ? <ResultBanner title={`Event bus ${bus} created.`} returned="CreateEventBus returned its ARN, and the bus's page opens." onDismiss={() => go("/resources/$service/$kind/$name", {})} testId="created-banner" /> : undefined}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        overview={
          <>
            {groups.map((g, i) => <FlowStrip key={i} groups={g} className="mb-4" />)}
            <div className="mt-6 mb-10">
              <Heading title="Event bus details" />
              <DetailsGrid columns={2} testId="bus-details" rows={[["Name", <span key="n" className="font-mono">{bus}</span>], ["ARN", <span key="a" className="font-mono">{arn}</span>], ["Description", field(d, "Description") ?? <span className="text-muted-foreground">–</span>], ["Encryption", kms && kms !== "AWS owned" ? <span key="k" className="font-mono">{kms}</span> : "AWS owned key"], ["Dead-letter queue", dlq ? <span key="q" className="font-mono">{arnName(dlq)}</span> : "None"], ["Tags", tags.data?.tags.length ? <span key="t" className="flex flex-wrap gap-1.5">{tags.data.tags.map((t) => <Badge key={t.key} variant="outline" className="font-mono text-[11.5px] font-normal">{t.value ? `${t.key}=${t.value}` : t.key}</Badge>)}</span> : <span className="text-muted-foreground">None</span>]]} />
            </div>
            <section aria-labelledby="archives-title" data-testid="bus-archives">
              <Heading title="Archives" id="archives-title">Archives that receive events from this bus.</Heading>
              <DataTable<ArchiveItem> aria-label="Archives" data={all} columns={columns} rowId={(a) => a.ArchiveName} empty={`No archive keeps the events of ${bus} yet.`} />
              <p className="mt-3 text-[13px] text-muted-foreground">Columns follow the fields ListArchives returns.</p>
              <div className="mt-3 flex flex-wrap items-center gap-3">
                <Button variant="outline" className="bg-card" onClick={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "archives" }, search: { create: "yes", name: bus } })} data-testid="archive-create"><PlusIcon /> Create archive</Button>
                <Button asChild variant="ghost"><Link to="/resources/$service/$second" params={{ service: "eventbridge", second: "archives" }} search={{}}>View all archives</Link></Button>
              </div>
              <RuleHelp className="mt-4 max-w-3xl"><b>Retention is applied.</b> doze-aws removes the events older than an archive's retention when the next event arrives, at most once an hour; an archive with no retention keeps its events until you delete it.</RuleHelp>
            </section>
            <RecentCalls className="mt-12" calls={calls} noun="bus" title="Recent calls" scope={`${bus} and its rules`} note={note} />
          </>
        }
        testId="bus-page"
      />
      <BusDelete open={deleting} onOpenChange={setDeleting} bus={bus} rules={ruleItems.map((r) => r.Name)} />
    </>
  );
}
