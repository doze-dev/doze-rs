// The exports (cloudformation__exports, exports-none) and the page of one (export-detail): the value, the stack that makes it, the
// stacks that import it, and what that means for deleting the stack.
import { Link } from "@tanstack/react-router";
import { PackageIcon } from "lucide-react";
import { useMemo } from "react";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { DetailError, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import type { OwnProps } from "../deep/registry";
import { ResourceTo } from "../value";
import { useCfnRead, useImporters } from "./api";
import { nameList, stackNameOf } from "./model";
import { Nav } from "./nav";
import { Facts, Lede, StackTo } from "./parts";
import { exportsSaid } from "./sentences";

type Row = { name: string; value: string; stack: string };

const mono = "font-mono text-[13px]";
const COLUMNS: DataColumn<Row>[] = [
  { id: "n", header: "Export name", size: 330, min: 200, accessorFn: (r) => r.name.toLowerCase(), cell: (c) => <ResourceTo to={{ service: "cloudformation", kind: "exports", name: c.row.original.name }} className={`${mono} font-medium break-all whitespace-normal`} testId="export-link">{c.row.original.name}</ResourceTo> },
  { id: "v", header: "Value", size: 440, min: 200, accessorKey: "value", cell: (c) => <span className={`${mono} break-all whitespace-normal`}>{c.row.original.value}</span> },
  { id: "s", header: "Exporting stack", accessorKey: "stack", cell: (c) => <StackTo name={c.row.original.stack} /> },
];

export function ExportsListPage({ service, kind }: OwnProps) {
  const list = useResourceList(service, kind);
  const first = list.data?.pages[0];
  const rows = useMemo<Row[]>(() => (list.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => ({ name: r.name, value: r.cells[0] ?? "", stack: stackNameOf(r.cells[1] ?? "") })), [list.data]);
  const importers = useImporters(useMemo(() => rows.map((r) => r.name), [rows]));
  const refresh = () => void list.refetch();
  const example = rows.find((r) => (importers.data?.[r.name] ?? []).length === 0) ?? rows[0];
  return (
    <>
      <PageHeader
        title="Exports"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub="Output values that other stacks in this account and Region can import. A stack exports a value with the Export field of an output."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
      />
      {list.isError && !first ? (
        <ErrorBlock what="The exports could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <Skeleton className="h-64 rounded-xl" data-testid="list-loading" />
      ) : rows.length === 0 ? (
        <div data-testid="list-empty">
          <Nav active="exports" />
          <EmptyState icon={PackageIcon} title="No exports">No stack in this region exports an output. A stack exports an output with the Export field so other stacks can import it.</EmptyState>
        </div>
      ) : (
        <>
          {importers.data && <Lede said={exportsSaid(rows, importers.data)} className="mb-5" testId="list-sentence" bold={[/\b\d+ (?:exports?|stacks?)\b/]} />}
          <Nav active="exports" />
          <DataTable aria-label="Exports" data={rows} columns={COLUMNS} rowId={(r) => r.name} rowHeight={52} maxHeight={560} />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "export" : "exports"} />
          <section className="mt-10 grid max-w-[56rem] gap-3" aria-label="Use an export in another template" data-testid="use-export">
            <div><h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Use an export in another template</h2><p className="mt-1 text-[13.5px] text-muted-foreground">Import it by name with Fn::ImportValue.</p></div>
            <RuleHelp>Export names must be unique among the exports of this account in this Region. Choose an export name to see which stacks import it.{example && (importers.data?.[example.name] ?? []).length === 0 ? ` Nothing imports ${example.name} today.` : ""}</RuleHelp>
            {example && <CodeBlock text={`Fn::ImportValue: ${example.name}`} max="max-h-24" testId="import-snippet" />}
          </section>
        </>
      )}
    </>
  );
}

export function ExportPage({ service, kind, name }: OwnProps) {
  const exportName = name ?? "";
  const all = useCfnRead<{ Exports?: { Name: string; Value: string; ExportingStackId: string }[] }>("ListExports", {});
  const found = all.data?.Exports?.find((e) => e.Name === exportName);
  const importers = useImporters(found ? [exportName] : []);
  if (all.isError) return <DetailError service={service} kind={kind} name={exportName} what="This export could not be read" error={all.error} onRetry={() => void all.refetch()} retrying={all.isFetching} />;
  if (all.isPending) return <Skeleton className="h-64 rounded-[14px]" data-testid="detail-loading" />;
  if (!found) return <DetailError service={service} kind={kind} name={exportName} error={new ApiFailure(404, { code: "ValidationError", message: `Export '${exportName}' does not exist.` })} />;
  const stack = stackNameOf(found.ExportingStackId);
  const users = importers.data?.[exportName];
  const others = (all.data?.Exports ?? []).filter((e) => e.Name !== exportName);
  const rows = (users ?? []).map((s) => ({ stack: s }));
  const columns: DataColumn<{ stack: string }>[] = [{ id: "s", header: "Stack name", accessorKey: "stack", cell: (c) => <StackTo name={c.row.original.stack} /> }];
  const said = users === undefined ? null : users.length === 0 ? `No stack imports this export, so ${stack} can be deleted and the output changed freely.` : `${users.length} ${users.length === 1 ? "stack imports" : "stacks import"} this export: ${nameList(users)}. Until ${users.length === 1 ? "it stops" : "they stop"}, the output can't change and ${stack} can't be deleted.`;
  return (
    <div data-testid="export-page">
      <PageHeader
        title={exportName}
        mono
        crumbs={[{ label: "Resources", to: "/resources" }, { label: "Infrastructure", to: "/resources" }, { label: "CloudFormation", to: `/resources/${service}` }, { label: "Exports", to: `/resources/${service}/${kind}` }]}
        sub={<>An exported output of <StackTo name={stack} className="font-sans text-[15px] font-normal text-foreground no-underline" />, read by other stacks with Fn::ImportValue.</>}
      />
      {said && <Lede said={{ text: said, names: [stack, ...(users ?? [])] }} testId="export-sentence" bold={[/^\d+ stacks?/]} />}
      <div className="grid gap-8">
        <Facts title="Export" testId="export-facts" wide={[{ label: "Export Name", value: exportName, mono: true }, { label: "Exporting stack", value: <StackTo name={stack} /> }, { label: "Value", value: found.Value, mono: true }]} rows={[]} />
        <section className="grid gap-3" aria-label="Importing stacks" data-testid="importers">
          <div><h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Importing stacks</h2><p className="mt-1 text-[13.5px] text-muted-foreground">Stacks whose templates use Fn::ImportValue on this name.</p></div>
          {users === undefined ? <Skeleton className="h-24 rounded-[12px]" /> : rows.length === 0 ? <EmptyTab title="No stack imports this export" testId="importers-none">Nothing uses {exportName} yet.</EmptyTab> : <DataTable aria-label="Importing stacks" data={rows} columns={columns} rowId={(r) => r.stack} rowHeight={48} maxHeight={320} />}
          <RuleHelp className="max-w-[56rem]">
            {users && users.length > 0 ? <>To delete {stack} or change this output, first remove the import from {nameList(users)}'s template and update {users.length === 1 ? "that stack" : "those stacks"}. DeleteStack on the exporting stack is cancelled while any import remains: the stack keeps its status and says why. </> : "Once a stack imports this export, the output can't change and the exporting stack can't be deleted. "}
            {others.length > 0 ? `${others.length === 1 ? "The other export is" : `The other ${others.length} exports are`} unaffected.` : ""}
          </RuleHelp>
        </section>
        <Link to="/resources/$service/$kind/$name" params={{ service, kind: "stacks", name: stack }} search={{ tab: "outputs" }} className="w-fit text-[14px] text-link hover:underline" data-testid="to-outputs">See {stack}'s outputs</Link>
      </div>
      {importers.isError && <ErrorBlock what="Who imports this export could not be read" error={importers.error} hint={HINT} testId="importers-error" />}
    </div>
  );
}
