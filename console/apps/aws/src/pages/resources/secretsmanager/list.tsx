// Secrets Manager's secrets (secretsmanager__secrets-list, secrets-none): the secrets with their descriptions, searched by the filters
// ListSecrets takes (name, description, tag key, tag value, any text), with scheduled-for-deletion secrets shown only when asked
// (Preferences). Store a new secret opens the wizard (`?create=yes`).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { LockKeyholeIcon, PlusIcon, SettingsIcon } from "lucide-react";
import { useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useAllRows } from "../cfg-api";
import { Note, Say } from "../cfg-parts";
import { FilterBar, type FilterProp } from "../filter-bar";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { SecretCreate } from "./create";
import { listSentence, PROPS, type SecretRow, secretRows, type Token, tokensFromQuery, tokensToQuery } from "./model";
import { FromYourApp } from "../../../kit/from-your-app";

const FILTERS: FilterProp[] = PROPS.map((p) => ({ value: p.value, label: p.label }));

export function SecretsList() {
  const search = useSearch({ strict: false }) as { q?: string; create?: string; mode?: string; do?: string; name?: string };
  if (search.create === "yes") return <SecretCreate />;
  return <Secrets q={search.q ?? ""} deleted={search.mode === "deleted"} stored={search.do === "stored" ? (search.name ?? null) : null} />;
}

function Secrets({ q, deleted, stored }: { q: string; deleted: boolean; stored: string | null }) {
  const navigate = useNavigate();
  const region = useAbout().data?.region ?? "this Region";
  const tokens = useMemo(() => tokensFromQuery(q), [q]);
  const list = useAllRows("secretsmanager", "secrets", { params: { ...(q ? { filter: q } : {}), ...(deleted ? { deleted: "yes" } : {}) } });
  const everything = useAllRows("secretsmanager", "secrets", { params: deleted ? { deleted: "yes" } : {}, enabled: q !== "" });
  const rows = useMemo(() => secretRows(list.rows, list.columns), [list.rows, list.columns]);
  const first = list.first;
  const go = (next: { q?: string; mode?: string; create?: "yes" }) => void navigate({ to: "/resources/$service/$second", params: { service: "secretsmanager", second: "secrets" }, search: next });
  const keep = (over: { tokens?: Token[]; deleted?: boolean }) => {
    const t = tokensToQuery(over.tokens ?? tokens);
    const d = over.deleted ?? deleted;
    go({ ...(t ? { q: t } : {}), ...(d ? { mode: "deleted" } : {}) });
  };
  const refresh = () => { void list.q.refetch(); };
  const none = rows.length === 0 && q === "" && !deleted;
  const columns: DataColumn<SecretRow>[] = [
    { id: "name", header: "Secret name", min: 220, accessorKey: "name", meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "secretsmanager", kind: "secrets", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "description", header: "Description", accessorKey: "description", cell: (c) => <span className="flex items-center gap-2">{c.row.original.description || <span className="text-muted-foreground">–</span>}{c.row.original.scheduled && <Badge variant="outline" className="border-warn/40 bg-warn/10 font-normal text-warn">Scheduled for deletion</Badge>}</span> },
  ];
  const create = <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Store a new secret</Button>;
  return (
    <>
      <PageHeader title="Secrets" crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: "/resources/secretsmanager" }] : [])]} sub={`Credentials, API keys and other secrets your apps read at run time. Secrets Manager is regional: this list holds the secrets in ${region}.`} refresh={<RefreshControl updatedAt={list.q.dataUpdatedAt} fetching={list.q.isFetching} onRefresh={refresh} interval={false} />} actions={create} />
      {stored && <ResultBanner className="mb-6" title={`Secret ${stored} stored.`} returned="CreateSecret returned its ARN and the id of its first version, which has the label AWSCURRENT." onDismiss={() => keep({})} testId="stored-banner" />}
      {list.q.isError && !first ? (
        <ErrorBlock what="The secrets could not be listed" error={list.q.error} hint={HINT} onRetry={refresh} retrying={list.q.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={LockKeyholeIcon} title="No secrets" action={create}>Store a secret to keep a database password or an API key out of your code.</EmptyState>
          <FromYourApp service="secretsmanager" />
        </div>
      ) : (
        <>
          <Say text={listSentence(rows, q !== "", everything.rows.length || null)} className="mb-5" testId="list-sentence" />
          <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
            <FilterBar props={FILTERS} tokens={tokens} onChange={(t) => keep({ tokens: t })} label="Search secrets" placeholder="Type to search" />
            <DropdownMenu>
              <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="preferences"><SettingsIcon /> Preferences</Button></DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-64">
                <DropdownMenuCheckboxItem checked={deleted} onCheckedChange={(v) => keep({ deleted: v === true })} data-testid="show-deleted">Show secrets scheduled for deletion</DropdownMenuCheckboxItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          {rows.length === 0 ? (
            <EmptyState icon={LockKeyholeIcon} title="No secret matches" action={<Button variant="outline" className="bg-card" onClick={() => keep({ tokens: [] })} data-testid="filters-clear">Clear filters</Button>}>ListSecrets returned none for these filters. The Name filter matches the start of a name and is case-sensitive; the others match words, whatever the case.</EmptyState>
          ) : (
            <>
              <DataTable aria-label="Secrets" data={rows} columns={columns} rowId={(r) => r.name} maxHeight={680} />
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.more} noun={rows.length === 1 ? "secret" : "secrets"} />
            </>
          )}
          <Note className="mt-5" testId="list-note">Searching without a filter matches words in the name, description, tag keys and tag values, ignoring case and special characters. The Name filter matches the start of a name and is case-sensitive: <span className="font-mono">larder/prod/</span> finds both of those secrets, <span className="font-mono">Larder/prod/</span> finds none. Secrets scheduled for deletion are not listed; open Preferences and select Show secrets scheduled for deletion to see them.</Note>
        </>
      )}
    </>
  );
}
