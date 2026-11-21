// EventBridge's rules (eventbridge__rules-list, rules-empty, rule-delete from the list): a bus at a time, the pattern and the targets in the row, rows
// chosen for Disable, Enable and Delete. `?name=<bus>` is the bus on screen, `?create=yes` the wizard, `?do=schedule` the scheduled one.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { InboxIcon, PlusIcon, TrashIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { FilterChips } from "@doze/ui/components/app/filter-chips";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { RuleHelp, callKindOp, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { RULES, busArn, ruleAddress, useArchives, useBuses, useRulesOf, useTargetsOf } from "./api";
import { Said, State, useWhere } from "./parts";
import { RuleCreate } from "./rule-create";
import { RuleDelete } from "./rule-delete";
import { SectionStrip } from "./strip";
import type { RuleItem, TargetItem } from "./types";
import { count, patternWords, rulesSentence, targetParts } from "./words";
import { Link } from "@tanstack/react-router";

export function RulesList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; do?: string; name?: string; gone?: string };
  if (search.create === "yes" || search.do === "schedule") return <RuleCreate schedule={search.do === "schedule"} bus={search.name} />;
  return <Rules service={service} kind={kind} bus={search.name} gone={search.gone} />;
}

type Row = RuleItem & { targets: TargetItem[] | undefined };

function Rules({ service, kind, bus: asked, gone }: { service: string; kind: string; bus?: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const { region, account } = useWhere();
  const buses = useBuses();
  const names = (buses.data?.EventBuses ?? []).map((b) => b.Name);
  const bus = asked && (names.length === 0 || names.includes(asked)) ? asked : (names.includes("default") ? "default" : (names[0] ?? "default"));
  const rules = useRulesOf(bus);
  const archives = useArchives(busArn(region, account, bus));
  const items = useMemo(() => rules.data?.Rules ?? [], [rules.data]);
  const targets = useTargetsOf(bus, items);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const [acting, setActing] = useState<string | null>(null);
  const [actError, setActError] = useState<unknown>(null);
  const rows: Row[] = useMemo(() => items.map((r) => ({ ...r, targets: targets[r.Name] })), [items, targets]);
  const picked = rows.filter((r) => chosen.has(r.Name));
  const go = (search: { create?: "yes"; do?: string; name?: string; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const refresh = useCallback(() => { void rules.refetch(); void buses.refetch(); }, [rules, buses]);
  const setState = async (operation: "EnableRule" | "DisableRule") => {
    setActing(operation);
    setActError(null);
    try {
      for (const r of picked) await callKindOp(RULES, operation, { Name: r.Name, EventBusName: bus });
      await rules.refetch();
      setChosen(new Set());
    } catch (e) {
      setActError(e);
    } finally {
      setActing(null);
    }
  };
  const columns: DataColumn<Row>[] = [
    { id: "name", header: "Name", min: 180, accessorFn: (r) => r.Name, cell: (c) => <ResourceTo to={{ service, kind, name: ruleAddress(bus, c.row.original.Name) }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.Name}</ResourceTo> },
    { id: "state", header: "State", size: 130, accessorFn: (r) => r.State ?? "", cell: (c) => <State state={c.row.original.State} /> },
    { id: "pattern", header: "Event pattern", size: 280, accessorFn: (r) => patternWords(r.EventPattern, r.ScheduleExpression), cell: (c) => <PatternCell r={c.row.original} /> },
    { id: "targets", header: "Targets", size: 260, accessorFn: (r) => r.targets?.length ?? 0, cell: (c) => <TargetsCell t={c.row.original.targets} /> },
    { id: "description", header: "Description", min: 160, accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description ?? <span className="text-muted-foreground">–</span> },
  ];
  const mine = archives.data?.Archives ?? [];
  const sentence = rulesSentence(bus, items, targets);
  const refused = actError instanceof Error ? actError.message : null;
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="Rule deleted." returned={<>Removed the targets of <b className="font-mono font-semibold text-bright">{gone}</b>, then deleted it from <b className="font-mono font-semibold text-bright">{bus}</b>. The resources its targets sent to are untouched.</>} onDismiss={() => go({ name: bus })} testId="deleted-banner" />}
      <PageHeader
        title="Rules"
        crumbs={[...crumbs]}
        belowCrumbs={<SectionStrip current="rules" />}
        sub={<><b className="font-semibold text-foreground">A rule watches one event bus.</b> Events that match its pattern go to its targets, up to five of them.</>}
        refresh={<RefreshControl updatedAt={rules.dataUpdatedAt} fetching={rules.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes", name: bus })} data-testid="create-open"><PlusIcon /> Create rule</Button>}
      />
      {rules.isError ? (
        <ErrorBlock what="The rules could not be listed" error={rules.error} hint={HINT} onRetry={refresh} retrying={rules.isFetching} testId="list-error" />
      ) : !rules.data || !buses.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : (
        <>
          <Said text={sentence} names={[bus, ...items.map((r) => r.Name)]} bold={[/\b\d+(?= (?:rules?|targets?)\b)/, /\bno rules\b/]} />
          <div className="mb-4 flex flex-wrap items-center gap-3" data-testid="bus-chips">
            <span className="text-[13.5px] text-muted-foreground">Event bus</span>
            <FilterChips chips={names.map((n) => ({ value: n, label: n }))} value={bus} onChange={(n) => { setChosen(new Set()); go({ name: n }); }} label="Event bus" />
          </div>
          {items.length === 0 ? (
            <div data-testid="list-empty">
              <EmptyState icon={InboxIcon} title={`No rules on ${bus}`} action={<Button onClick={() => go({ create: "yes", name: bus })}><PlusIcon /> Create rule</Button>}>
                A rule sends the events that match its pattern to up to five targets. Until this bus has one, no rule delivers what it receives.
              </EmptyState>
              <RuleHelp className="mt-6 max-w-3xl">Rules only see events sent to their own bus. {mine.length > 0 ? <><b className="font-mono font-semibold text-foreground">{bus}</b> has {mine.length === 1 ? "an archive" : "archives"}, <span className="font-mono">{mine.map((a) => a.ArchiveName).join(", ")}</span>, {mine.every((a) => !a.EventPattern) ? "with no filter, so it keeps every event it receives even with no rule." : "which keep the events their patterns name, with or without a rule."}</> : <><b className="font-mono font-semibold text-foreground">{bus}</b> has no archive, so an event sent to it is dropped.</>}</RuleHelp>
            </div>
          ) : (
            <>
              {refused && <p className="mb-3 text-[13px] text-bad" role="alert">{refused}</p>}
              <SelectableTable<Row>
                aria-label="Rules"
                data={rows}
                columns={columns}
                rowId={(r) => r.Name}
                selected={chosen}
                onSelectedChange={setChosen}
                noun="rule"
                maxHeight={680}
                rowHeight={58}
                actionsAlways
                actionsEnd
                hint=" "
                actions={(sel) => {
                  const on = picked.some((r) => r.State === "ENABLED");
                  return (
                    <>
                      <Button type="button" variant="outline" className="bg-card" disabled={sel.length === 0 || acting !== null} onClick={() => void setState(on ? "DisableRule" : "EnableRule")} data-testid="action-toggle">{on || sel.length === 0 ? "Disable" : "Enable"}</Button>
                      <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length === 0} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>
                    </>
                  );
                }}
              />
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={Boolean(rules.data.NextToken)} noun={rows.length === 1 ? "rule" : "rules"} />
              <p className="mt-4 max-w-3xl text-[13px] text-muted-foreground">Columns follow the fields ListRules returns (Name, State, EventPattern, Description); Targets comes from ListTargetsByRule.</p>
            </>
          )}
          <p className="mt-6 flex flex-wrap items-center gap-3 text-[13.5px] text-muted-foreground">
            Scheduled rules (legacy) run on the default bus. For new schedules AWS recommends <Link to="/resources/$service/$second" params={{ service: "scheduler", second: "schedules" }} search={{}} className="text-link hover:underline">EventBridge Scheduler</Link>, which doze-aws has too.
            <Button variant="outline" size="sm" className="bg-card" onClick={() => go({ do: "schedule" })} data-testid="create-schedule">Create scheduled rule</Button>
          </p>
          <RuleHelp className="mt-6 max-w-3xl">A bus can hold <b>300</b> rules by default on AWS, and a rule can have <b>5</b> targets. A disabled rule matches no events. Changes can take a short while to apply on AWS; here they apply at once.</RuleHelp>
        </>
      )}
      {picked.length > 0 && (
        <RuleDelete open={asking} onOpenChange={setAsking} bus={bus} rules={picked.map((r) => ({ name: r.Name, targets: r.targets ?? [] }))} onDone={(done) => { setAsking(false); setChosen(new Set()); go({ name: bus, gone: done.join(", ") }); }} />
      )}
    </>
  );
}

function PatternCell({ r }: { r: Row }) {
  if (r.ScheduleExpression) return <span className="font-mono text-[12.5px]">{r.ScheduleExpression}</span>;
  const words = patternWords(r.EventPattern);
  const [first, ...rest] = words.split(" ");
  return words === "–" || words === "Custom pattern" ? <span className="text-muted-foreground">{words}</span> : <div className="grid min-w-0 leading-5"><span className="truncate font-mono text-[12.5px]">{first}</span>{rest.length > 0 && <span className="truncate text-[12.5px] text-muted-foreground">{rest.join(" ")}</span>}</div>;
}

function TargetsCell({ t }: { t: TargetItem[] | undefined }) {
  if (t === undefined) return <span className="text-muted-foreground">…</span>;
  if (t.length === 0) return <span className="text-muted-foreground">No targets</span>;
  const p = targetParts(t[0]!.Arn);
  const more = t.length > 1 ? ` and ${count(t.length - 1, "more")}` : "";
  return <div className="grid min-w-0 leading-5"><span className="truncate text-[13px]">{p.kind}{more}</span><span className="truncate font-mono text-[12px] text-muted-foreground">{p.name}</span></div>;
}
