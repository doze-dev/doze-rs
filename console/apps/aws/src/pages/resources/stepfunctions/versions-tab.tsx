// A state machine's Versions and Aliases tabs (stepfunctions__machine-versions, machine-versions-none, machine-aliases,
// machine-aliases-none): the immutable snapshots and the pointers to them, each chosen by a radio so Start execution starts it.
import { Link } from "@tanstack/react-router";
import { PlayIcon, PlusIcon, SearchIcon, TagIcon, WorkflowIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { Loaded } from "../deep/parts";
import { at } from "../deep/parts";
import { StartDialog } from "./dialogs";
import { fieldOf } from "./details-card";
import { type AliasRow, type VersionRow, useAliases, useVersions } from "./hooks";
import { PublishDialog } from "./publish-dialog";
import { RadioRows, radioColumn } from "./radio";
import { inputSkeleton, routingWords } from "./model";
import { parseDefinition } from "./graph";
import { useGoTo } from "./hooks";

const dash = <span className="text-muted-foreground">-</span>;

const VERSION_COLUMNS: DataColumn<VersionRow>[] = [
  radioColumn<VersionRow>((r) => r.number, "version"),
  { id: "v", header: "Version", size: 110, accessorFn: (r) => Number(r.number), meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright" data-testid="version-cell">{c.row.original.number}</span> },
  { id: "d", header: "Description", min: 200, accessorFn: (r) => r.description, cell: (c) => c.row.original.description || dash },
  { id: "c", header: "Creation date", size: 220, accessorFn: (r) => String(r.created), cell: (c) => at(c.row.original.created) },
];

export function VersionsTab({ d }: { d: ResourceDetail }) {
  const versions = useVersions(d.name);
  const go = useGoTo();
  const [filter, setFilter] = useState("");
  const [chosen, setChosen] = useState<string | null>(null);
  const [publishing, setPublishing] = useState(false);
  const [starting, setStarting] = useState(0);
  const startAt = parseDefinition(fieldOf(d, "Definition") ?? "")?.StartAt;
  return (
    <Loaded q={versions} what="The versions">
      {(list) => {
        if (list.length === 0) {
          return (
            <>
              <EmptyTab icon={WorkflowIcon} title="No versions" action={<Button type="button" variant="outline" className="bg-card" onClick={() => setPublishing(true)} data-testid="publish-open">Publish version</Button>} testId="versions-none">
                {d.name} has no published versions. A version is an immutable snapshot of its definition.
              </EmptyTab>
              <PublishDialog d={d} open={publishing} onOpenChange={setPublishing} done={() => { setPublishing(false); go.machine(d.name, { tab: "versions", done: "published" }); }} />
            </>
          );
        }
        const rows = [...list].sort((a, b) => Number(b.number) - Number(a.number)).filter((r) => `${r.number} ${r.description}`.toLowerCase().includes(filter.trim().toLowerCase()));
        const pick = list.find((r) => r.number === chosen) ?? rows[0] ?? null;
        return (
          <>
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <div className="relative w-full max-w-[19rem]">
                <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter by property or value" aria-label="Filter versions" className="bg-card pl-9" data-testid="versions-filter" />
              </div>
              <Button type="button" variant="outline" className="bg-card" disabled={!pick} onClick={() => setStarting((n) => n + 1)} data-testid="version-start"><PlayIcon /> Start execution</Button>
            </div>
            <RadioRows value={pick?.number ?? null} onChange={setChosen}>
              <DataTable aria-label="Versions" data={rows} columns={VERSION_COLUMNS} rowId={(r) => r.number} maxHeight={420} rowClassName={(r) => (r.number === pick?.number ? "[&>td]:bg-accent" : undefined)} onRowClick={(r) => setChosen(r.number)} empty="No version matches." />
            </RadioRows>
            <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="versions-note">
              A version is a numbered, immutable snapshot of a revision. Numbers start at 1 and are never reused: delete version {Math.max(...list.map((v) => Number(v.number)))} and the next one published is {Math.max(...list.map((v) => Number(v.number))) + 1}. AWS allows up to 1,000 versions per state machine; doze-aws does not enforce that limit. Versions are published from <Link to="/resources/$service/$kind/$name" params={{ service: "stepfunctions", kind: "machines", name: d.name }} search={{ do: "edit" }} className="font-medium text-link hover:underline">Edit</Link>; starting one from here uses its version ARN.
            </p>
            {pick && starting > 0 && (
              <StartDialog key={`${pick.number}-${starting}`} machine={d.id} name={d.name} startAt={startAt} input={inputSkeleton(parseDefinition(fieldOf(d, "Definition") ?? ""))} open onOpenChange={() => setStarting(0)} target={{ arn: pick.arn, words: <>Starts <b className="font-semibold">version {pick.number}</b> of <span className="font-mono font-semibold text-bright">{d.name}</span> by its version ARN. The execution runs the definition that version holds.</> }} />
            )}
          </>
        );
      }}
    </Loaded>
  );
}

const ALIAS_COLUMNS: DataColumn<AliasRow>[] = [
  radioColumn<AliasRow>((r) => r.name, "alias"),
  { id: "n", header: "Alias", size: 160, accessorFn: (r) => r.name, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright" data-testid="alias-cell">{c.row.original.name}</span> },
  { id: "d", header: "Description", size: 260, min: 160, accessorFn: (r) => r.description, cell: (c) => c.row.original.description || dash },
  { id: "r", header: "Routing configuration", size: 260, min: 170, fit: 170, accessorFn: (r) => routingWords(r.routing), cell: (c) => routingWords(c.row.original.routing) },
  { id: "c", header: "Creation date", size: 190, fit: 112, accessorFn: (r) => String(r.created), cell: (c) => at(c.row.original.created) },
  { id: "u", header: "Update date", size: 190, fit: 106, accessorFn: (r) => String(r.updated), cell: (c) => at(c.row.original.updated) },
];

export function AliasesTab({ d }: { d: ResourceDetail }) {
  const aliases = useAliases(d.name);
  const go = useGoTo();
  const [chosen, setChosen] = useState<string | null>(null);
  const [starting, setStarting] = useState(0);
  const startAt = parseDefinition(fieldOf(d, "Definition") ?? "")?.StartAt;
  const make = () => go.machine(d.name, { do: "alias-create" });
  const rows = useMemo(() => aliases.data ?? [], [aliases.data]);
  const pick = rows.find((r) => r.name === chosen) ?? rows[0] ?? null;
  return (
    <Loaded q={aliases} what="The aliases">
      {() =>
        rows.length === 0 ? (
          <EmptyTab icon={TagIcon} title="No aliases" action={<Button type="button" variant="outline" className="bg-card" onClick={make} data-testid="alias-create-empty">Create alias</Button>} testId="aliases-none">
            {d.name} has no aliases. An alias points to up to two versions.
          </EmptyTab>
        ) : (
          <>
            <div className="mb-4 flex flex-wrap items-center justify-end gap-3">
              <Button type="button" variant="outline" className="bg-card" disabled={!pick} onClick={() => setStarting((n) => n + 1)} data-testid="alias-start"><PlayIcon /> Start execution</Button>
              <Button type="button" variant="outline" className="bg-card" onClick={make} data-testid="alias-create"><PlusIcon /> Create new alias</Button>
            </div>
            <RadioRows value={pick?.name ?? null} onChange={setChosen}>
              <DataTable aria-label="Aliases" data={rows} columns={ALIAS_COLUMNS} rowId={(r) => r.name} maxHeight={420} rowClassName={(r) => (r.name === pick?.name ? "[&>td]:bg-accent" : undefined)} onRowClick={(r) => setChosen(r.name)} />
            </RadioRows>
            <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="aliases-note">
              An alias points at one version, or splits executions between two by percentage. Starting an execution from an alias records both the alias and the version it chose. AWS allows up to 100 aliases per state machine; doze-aws does not enforce that limit.
            </p>
            {pick && starting > 0 && (
              <StartDialog key={`${pick.name}-${starting}`} machine={d.id} name={d.name} startAt={startAt} input={inputSkeleton(parseDefinition(fieldOf(d, "Definition") ?? ""))} open onOpenChange={() => setStarting(0)} target={{ arn: pick.arn, words: <>Starts the alias <b className="font-mono font-semibold">{pick.name}</b> of <span className="font-mono font-semibold text-bright">{d.name}</span>, which picks a version by its routing: {routingWords(pick.routing)}.</> }} />
            )}
          </>
        )
      }
    </Loaded>
  );
}
