// IAM's user groups (iam__groups-list, groups-none): each group with its members' count, the sentence of what the groups hold, and the
// button that makes one. `?create=yes` is Create user group, as a page.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, UsersIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useResourceList } from "../../../api/resources";
import { GROUPS, useEach, useRefresh } from "./api";
import { GroupCreate } from "./group-create";
import { CreatedBanner, GoneBanner, ListError, ListLoading, PageLine, SearchBox, useToList } from "./list-parts";
import { crumbs, Say, To } from "./parts";
import { groupsSentence } from "./sentences";
import { useGroupPolicies } from "./user-data";

type Held = { Users: { UserName: string }[]; Group: { CreateDate: number } };
type Row = { name: string; members: number | null; created: number | null };

export function GroupsList() {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string; do?: string; name?: string };
  if (search.create === "yes") return <GroupCreate />;
  return <Groups gone={search.gone} created={search.do === "created" ? search.name : undefined} />;
}

function Groups({ gone, created }: { gone?: string; created?: string }) {
  const navigate = useNavigate();
  const toList = useToList("groups");
  const refreshAll = useRefresh();
  const list = useResourceList("iam", "groups");
  const [typed, setTyped] = useState("");
  const names = useMemo(() => list.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [list.data]);
  const held = useEach<Held>(GROUPS, "GetGroup", names.map((GroupName) => ({ GroupName })));
  const { byGroup } = useGroupPolicies(names);
  const first = list.data?.pages[0];
  const { refetch } = list;
  const refresh = useCallback(() => {
    void refetch();
    refreshAll();
  }, [refetch, refreshAll]);
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "groups" }, search: { create: "yes" } });
  const all: Row[] = names.map((name, i) => ({ name, members: held[i]?.data ? held[i]!.data!.Users.length : null, created: held[i]?.data?.Group.CreateDate ?? null }));
  const rows = all.filter((r) => r.name.toLowerCase().includes(typed.trim().toLowerCase()));
  const sentence = groupsSentence(names.map((name, i) => ({ name, members: (held[i]?.data?.Users ?? []).map((u) => u.UserName), policies: (byGroup.get(name) ?? []).map((p) => p.PolicyName) })));
  const columns: DataColumn<Row>[] = [
    { id: "name", header: "Group name", size: 320, min: 180, accessorFn: (r) => r.name.toLowerCase(), cell: (c) => <To kind="groups" name={c.row.original.name} /> },
    { id: "users", header: "Users", size: 160, accessorFn: (r) => r.members ?? -1, meta: { numeric: true }, cell: (c) => c.row.original.members ?? <span className="text-muted-foreground">…</span> },
    { id: "created", header: "Created", accessorFn: (r) => r.created ?? 0, cell: (c) => (c.row.original.created ? dateTimeUtc(c.row.original.created * 1000) : <span className="text-muted-foreground">…</span>) },
  ];
  const none = names.length === 0 && first !== undefined;
  return (
    <>
      <PageHeader
        title="User groups"
        crumbs={crumbs()}
        sub="A group manages the permissions of several users with similar roles. Policies attached to the group apply to every member."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={create} data-testid="create-open"><PlusIcon /> Create group</Button>}
      />
      {gone && <GoneBanner kind="User group" name={gone} back={() => toList()} />}
      {created && <CreatedBanner kind="User group" noun="group" kindId="groups" name={created} returned="CreateGroup answered, and the group is in the list." back={() => toList()} />}
      {list.isError && !first ? (
        <ListError what="The user groups could not be listed" error={list.error} retry={refresh} retrying={list.isFetching} />
      ) : !first ? (
        <ListLoading />
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={UsersIcon} title="No user groups" action={<Button variant="outline" className="bg-card" onClick={create}><PlusIcon /> Create group</Button>}>
            A group lets you attach one set of permissions to many users.
          </EmptyState>
        </div>
      ) : (
        <>
          <Say text={sentence} names={[...names, ...held.flatMap((h) => (h.data?.Users ?? []).map((u) => u.UserName)), ...[...byGroup.values()].flat().map((p) => p.PolicyName)]} bold={[/^\d[\d,]* groups?/]} className="mb-6" testId="list-sentence" />
          <div className="mb-3"><SearchBox value={typed} onChange={setTyped} noun="groups" /></div>
          <DataTable aria-label="User groups" data={rows} columns={columns} rowId={(r) => r.name} maxHeight={680} empty={`No group's name contains “${typed}”.`} />
          <PageLine shown={names.length} noun="group" more={Boolean(list.hasNextPage)} loading={list.isFetchingNextPage} onMore={() => void list.fetchNextPage()} />
        </>
      )}
    </>
  );
}
