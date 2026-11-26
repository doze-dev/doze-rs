// IAM's roles (iam__roles-list, roles-none; Delete with a role chosen is iam__role-delete): each role with who may assume it, a sentence of
// what the record says of its sessions, and the checkbox that chooses the role Delete acts on. `?create=yes` is the Create role wizard.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, TrashIcon, UserRoundCogIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { usePrincipals } from "../../../api/spaces";
import { useNow } from "../../card";
import { assumedOf, ROLES, useIamCalls, useRead, useRefresh } from "./api";
import { DeleteEntity } from "./delete-entity";
import { CreatedBanner, GoneBanner, ListError, ListLoading, PageLine, SearchBox, useToList } from "./list-parts";
import { type Doc, trustedEntities } from "./model";
import { KINDS, listCrumbs, Say, To } from "./parts";
import { RoleCreate } from "./role-create";
import { rolesSentence } from "./sentences";

type RoleRow = { RoleName: string; CreateDate: number; AssumeRolePolicyDocument: Doc };

export function RolesList() {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string; do?: string; name?: string };
  if (search.create === "yes") return <RoleCreate />;
  return <Roles gone={search.gone} created={search.do === "created" ? search.name : undefined} />;
}

function Roles({ gone, created }: { gone?: string; created?: string }) {
  const navigate = useNavigate();
  const toList = useToList("roles");
  const refreshAll = useRefresh();
  const read = useRead<{ Roles: RoleRow[]; IsTruncated?: boolean }>(ROLES, "ListRoles", { MaxItems: 1000 });
  const calls = useIamCalls();
  const principals = usePrincipals("all");
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const now = useNow();
  const all = useMemo(() => read.data?.Roles ?? [], [read.data]);
  const rows = all.filter((r) => r.RoleName.toLowerCase().includes(typed.trim().toLowerCase()));
  const keyNames = useMemo(() => new Map((principals.data?.principals ?? []).filter((p) => p.kind === "user" && p.name).map((p) => [p.key, p.name as string])), [principals.data]);
  const sentence = rolesSentence(all.map((r) => { const a = assumedOf(calls.data?.calls ?? [], r.RoleName, keyNames, now); return { name: r.RoleName, trusted: trustedEntities(r.AssumeRolePolicyDocument), assumed: a.count, inLastHour: a.lastHour, assumedBy: a.by }; }));
  const { refetch } = read;
  const refresh = useCallback(() => {
    void refetch();
    refreshAll();
  }, [refetch, refreshAll]);
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "roles" }, search: { create: "yes" } });
  const one = chosen.size === 1 ? [...chosen][0]! : null;
  const columns: DataColumn<RoleRow>[] = [
    { id: "name", header: "Role name", size: 280, min: 170, accessorFn: (r) => r.RoleName.toLowerCase(), cell: (c) => <To kind="roles" name={c.row.original.RoleName} /> },
    { id: "trusted", header: "Trusted entities", size: 360, min: 200, accessorFn: (r) => trustedEntities(r.AssumeRolePolicyDocument).join(", "), cell: (c) => trustedEntities(c.row.original.AssumeRolePolicyDocument).join(", ") || <span className="text-muted-foreground">None</span> },
    { id: "created", header: "Created", accessorFn: (r) => r.CreateDate, cell: (c) => dateTimeUtc(c.row.original.CreateDate * 1000) },
  ];
  const none = read.isSuccess && all.length === 0;
  return (
    <>
      <PageHeader
        title="Roles"
        crumbs={listCrumbs(KINDS.roles)}
        sub="A role is an identity with specific permissions that anyone you trust can assume. Its credentials are temporary."
        refresh={<RefreshControl updatedAt={read.dataUpdatedAt} fetching={read.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            {!none && <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={one === null} title={one === null ? "Choose one role to delete" : undefined} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>}
            <Button onClick={create} data-testid="create-open"><PlusIcon /> Create role</Button>
          </>
        }
      />
      {gone && <GoneBanner kind="Role" name={gone} back={() => toList()} />}
      {created && <CreatedBanner kind="Role" noun="role" kindId="roles" name={created} returned="CreateRole answered with the trust policy you built, and the role is in the list." back={() => toList()} />}
      {read.isError && !read.data ? (
        <ListError what="The roles could not be listed" error={read.error} retry={refresh} retrying={read.isFetching} />
      ) : !read.data ? (
        <ListLoading />
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={UserRoundCogIcon} title="No roles" action={<Button variant="outline" className="bg-card" onClick={create}><PlusIcon /> Create role</Button>}>
            A role gives a service or a person temporary credentials with the permissions you choose.
          </EmptyState>
        </div>
      ) : (
        <>
          <Say text={sentence} names={all.map((r) => r.RoleName)} bold={[/^\d[\d,]* roles?/, /\b\d+ times?\b/]} className="mb-6" testId="list-sentence" />
          <SelectableTable aria-label="Roles" data={rows} columns={columns} rowId={(r) => r.RoleName} selected={chosen} onSelectedChange={setChosen} noun="role" toolbar={<SearchBox value={typed} onChange={setTyped} noun="roles" />} selectedLabel={(c) => `${c} selected`} maxHeight={680} empty={`No role's name contains “${typed}”.`} />
          <PageLine shown={all.length} noun="role" more={Boolean(read.data.IsTruncated)} loading={false} onMore={() => undefined} />
        </>
      )}
      {one && <DeleteEntity kind="roles" name={one} open={asking} onOpenChange={setAsking} onDeleted={() => { setAsking(false); setChosen(new Set()); toList({ gone: one }); }} />}
    </>
  );
}
