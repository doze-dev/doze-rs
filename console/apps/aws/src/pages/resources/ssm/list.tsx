// Parameter Store's parameters (ssm__parameters-list, parameters-staging, parameters-delete, parameters-deleted, parameter-created):
// the parameters searched by the filters DescribeParameters takes, ticked to Edit or Delete together (DeleteParameters, ten at a
// time). Create parameter opens the form (`?create=yes`); after a create or a delete the list says what the call returned.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PencilIcon, PlusIcon, SearchIcon, SlidersHorizontalIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useQueryClient } from "@tanstack/react-query";
import { useAbout } from "../../../api/queries";
import { callKindOp, PlainConfirm } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { andList, Note, Say } from "../cfg-parts";
import { FilterBar } from "../filter-bar";
import { HINT } from "../index-page";
import { refusalOf } from "../kms/schedule";
import { ResourceTo, Value } from "../value";
import { ParameterTabs, PublicParameters, ServiceSettings } from "./extra-tabs";
import { ParameterForm, PARAMETERS } from "./form";
import { FILTERS, filterSentence, listSentence, type ParamRow, paramRows, tokensFromQuery, tokensToQuery } from "./model";

export function SsmList() {
  const search = useSearch({ strict: false }) as { q?: string; create?: string; do?: string; name?: string; ver?: string; key?: string; mode?: string };
  if (search.create === "yes") return <ParameterForm />;
  if (search.mode === "public") return <PublicParameters />;
  if (search.mode === "settings") return <ServiceSettings />;
  return <Parameters q={search.q ?? ""} created={search.do === "created" && search.name ? { name: search.name, version: search.ver ?? "1", type: search.key ?? "" } : null} />;
}

type Deleted = { names: string[]; invalid: string[]; left: number };

function Parameters({ q, created }: { q: string; created: { name: string; version: string; type: string } | null }) {
  const navigate = useNavigate();
  const qc = useQueryClient();
  const region = useAbout().data?.region ?? "this Region";
  const tokens = useMemo(() => tokensFromQuery(q), [q]);
  const list = useAllRows("ssm", "parameters", { params: q ? { filter: q } : {} });
  const everything = useAllRows("ssm", "parameters", { enabled: q !== "" });
  const rows = useMemo(() => paramRows(list.rows, list.columns), [list.rows, list.columns]);
  const total = q !== "" ? everything.rows.length : rows.length;
  const first = list.first;
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [done, setDone] = useState<Deleted | null>(null);
  const picked = rows.filter((r) => chosen.has(r.name));
  const go = (search: { q?: string; create?: "yes" } = {}) => void navigate({ to: "/resources/$service/$second", params: { service: "ssm", second: "parameters" }, search });
  const filter = (t: Parameters<typeof tokensToQuery>[0]) => { setChosen(new Set()); const query = tokensToQuery(t); go(query ? { q: query } : {}); };
  const refresh = () => { void list.q.refetch(); };
  const del = async () => {
    setPending(true);
    setError(null);
    try {
      const names = picked.map((p) => p.name);
      const out: Deleted = { names: [], invalid: [], left: 0 };
      for (let i = 0; i < names.length; i += 10) {
        const r = await callKindOp<{ DeletedParameters?: string[]; InvalidParameters?: string[] }>(PARAMETERS, "DeleteParameters", { Names: names.slice(i, i + 10) });
        out.names.push(...(r.DeletedParameters ?? []));
        out.invalid.push(...(r.InvalidParameters ?? []));
      }
      out.left = Math.max(total - out.names.length, 0);
      setDone(out);
      setChosen(new Set());
      setConfirm(false);
      await qc.invalidateQueries({ queryKey: ["resources"] });
    } catch (e) {
      setError(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const columns: DataColumn<ParamRow>[] = [
    { id: "name", header: "Name", size: 310, min: 250, accessorKey: "name", meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "ssm", kind: "parameters", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "tier", header: "Tier", accessorKey: "tier", size: 100, fit: 70 },
    { id: "type", header: "Type", accessorKey: "type", size: 108, fit: 50 },
    { id: "version", header: "Version", accessorFn: (r) => Number(r.version), size: 72, fit: 70, meta: { numeric: true } },
    { id: "modified", header: "Last modified (UTC)", accessorFn: (r) => r.modified ?? "", size: 205, fit: 160, cell: (c) => <Value value={c.row.original.modified} format="time" /> },
    { id: "description", header: "Description", accessorKey: "description", min: 240, meta: { wrap: true } },
  ];
  const create = <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create parameter</Button>;
  const banner = done ? (
    <ResultBanner title="Parameters deleted." returned={<>DeleteParameters returned {done.names.length} deleted name{done.names.length === 1 ? "" : "s"}, <span className="font-mono">{andList(done.names)}</span>, and {done.invalid.length === 0 ? "no invalid parameters" : `${done.invalid.length} invalid parameter${done.invalid.length === 1 ? "" : "s"}: ${done.invalid.join(", ")}`}.</>} left={<>Now {done.left} parameter{done.left === 1 ? " is" : "s are"} left in {region}. AWS asks you to wait at least 30 seconds before creating a parameter with a deleted name.</>} tone={done.invalid.length > 0 ? "warn" : "good"} onDismiss={() => setDone(null)} testId="deleted-banner" />
  ) : created ? (
    <ResultBanner title="Parameter created." returned={<>PutParameter returned version {created.version} for <span className="font-mono">{created.name}</span>{created.type ? `, a ${created.type}` : ""}.</>} left={`There are now ${total} parameters in ${region}. Choose a name to check its Overview tab${created.type === "SecureString" ? "; on a SecureString, Show decrypts the value" : ""}.`} onDismiss={() => go()} testId="created-banner" />
  ) : null;
  return (
    <>
      <PageHeader title="Parameter Store" crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: "/resources/ssm" }] : [])]} sub={`A centralized configuration data store for named values called parameters. Parameters are only available in the Region where they were created (${region}).`} refresh={<RefreshControl updatedAt={list.q.dataUpdatedAt} fetching={list.q.isFetching} onRefresh={refresh} interval={false} />} actions={create} />
      <ParameterTabs current="parameters" />
      {banner && <div className="mb-6">{banner}</div>}
      {list.q.isError && !first ? (
        <ErrorBlock what="The parameters could not be listed" error={list.q.error} hint={HINT} onRetry={refresh} retrying={list.q.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : total === 0 && q === "" ? (
        <div data-testid="list-empty"><EmptyState icon={SlidersHorizontalIcon} title="No parameters" action={create}>Create a parameter to keep a setting or a secret out of your code.</EmptyState></div>
      ) : (
        <>
          <Say text={q === "" ? listSentence(rows, region) : filterSentence(rows, everything.rows.length || null)} names={rows.map((r) => r.name)} className="mb-5" testId="list-sentence" />
          <div className="mb-4"><FilterBar props={FILTERS} tokens={tokens} onChange={filter} label="Filter parameters" placeholder="Filter by name" /></div>
          {rows.length === 0 ? (
            <EmptyState icon={SearchIcon} title="0 parameters match" action={<Button variant="outline" className="bg-card" onClick={() => filter([])} data-testid="filters-clear">Remove the filter</Button>}>
              {tokens.length > 0 ? `No parameter name ${tokens[0]!.option === "Equals" ? "equals" : tokens[0]!.option === "Contains" ? "contains" : "begins with"} ${tokens[0]!.text}. ` : ""}Remove the filter to see the {total} parameter{total === 1 ? "" : "s"} that {total === 1 ? "is" : "are"} left.
            </EmptyState>
          ) : (
            <>
              <SelectableTable aria-label="Parameters" noun="parameter" data={rows} columns={columns} rowId={(r) => r.name} selected={chosen} onSelectedChange={setChosen} maxHeight={680} actionsAlways actionsEnd hint="Tick parameters to edit or delete them." actions={(p) => (
                <>
                  <Button type="button" variant="outline" className="bg-card" disabled={p.length !== 1} onClick={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "ssm", kind: "parameters", name: p[0]!.name }, search: { view: "edit" } })} data-testid="edit-selected"><PencilIcon /> Edit</Button>
                  <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={p.length === 0} onClick={() => setConfirm(true)} data-testid="delete-selected"><Trash2Icon /> Delete</Button>
                </>
              )} />
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.more} noun={rows.length === 1 ? "parameter" : "parameters"} />
            </>
          )}
          <Note className="mt-5" testId="list-note">{q !== "" ? <>Deleting removes every version of a parameter, and the parameter and its versions can&apos;t be restored. </> : null}The columns are the fields DescribeParameters returns for each parameter. The filter takes a name that equals, begins with or contains the text, and a type. The Public parameters tab lists what AWS publishes under /aws/service, and the Settings tab holds the settings of Parameter Store.</Note>
        </>
      )}
      <PlainConfirm open={confirm} onOpenChange={(o) => { if (!o) { setConfirm(false); setError(null); } }} title="Delete parameters" description="Deleting a parameter removes all versions of it. Once deleted, the parameter and its versions can't be restored." verb="Delete parameters" call="DeleteParameters" pending={pending} error={error ? new Error(`${(error as { code: string }).code}: ${(error as { message: string }).message}`) : undefined} onConfirm={() => void del()} testId="delete-parameters">
        <ul className="max-h-60 overflow-auto rounded-xl border bg-well" aria-label="Parameters to delete" data-testid="delete-names">
          <li className="border-b px-4 py-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">Name</li>
          {picked.map((p) => <li key={p.name} className="border-b px-4 py-2 font-mono text-[13px] last:border-b-0">{p.name}</li>)}
        </ul>
        <p className="text-[13px] leading-5 text-muted-foreground">After deleting a parameter, AWS asks you to wait at least 30 seconds before creating a parameter with the same name. One call deletes up to 10 names (DeleteParameters); names it can&apos;t delete come back as invalid parameters instead of failing the call.</p>
      </PlainConfirm>
    </>
  );
}
