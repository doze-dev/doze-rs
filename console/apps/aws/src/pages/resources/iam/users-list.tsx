// IAM's users (iam__users-list, users-none; Delete with a user chosen is iam__user-delete): each user with its access key, the key's age
// and when it was last used, a sentence of the account's users, and the checkbox that chooses the user Delete acts on.
// `?create=yes` is the Create user wizard.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, TrashIcon, UserIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { useResourceList } from "../../../api/resources";
import { useNow } from "../../card";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { useRefresh } from "./api";
import { DeleteEntity } from "./delete-entity";
import { CreatedBanner, GoneBanner, ListError, ListLoading, PageLine, SearchBox, useToList } from "./list-parts";
import { daysSince, sameDay } from "./model";
import { KINDS, listCrumbs, Say, To } from "./parts";
import { usersSentence } from "./sentences";
import { UserCreate } from "./user-create";
import { type UserKeys, useUserKeys } from "./user-data";

type Row = { name: string; keys: UserKeys | undefined };

export function UsersList() {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string; do?: string; name?: string };
  if (search.create === "yes") return <UserCreate />;
  return <Users gone={search.gone} created={search.do === "created" ? search.name : undefined} />;
}

function Users({ gone, created }: { gone?: string; created?: string }) {
  const navigate = useNavigate();
  const list = useResourceList("iam", "users");
  const toList = useToList("users");
  const refreshAll = useRefresh();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const names = useMemo(() => list.data?.pages.flatMap((p) => p.rows).map((r: ResourceRow) => r.name) ?? [], [list.data]);
  const { byUser, ready } = useUserKeys(names);
  const now = useNow();
  const rows: Row[] = names.filter((n) => n.toLowerCase().includes(typed.trim().toLowerCase())).map((name) => ({ name, keys: byUser.get(name) }));
  const first = list.data?.pages[0];
  const { refetch } = list;
  const refresh = useCallback(() => {
    void refetch();
    refreshAll();
  }, [refetch, refreshAll]);
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "users" }, search: { create: "yes" } });
  const chosenName = chosen.size === 1 ? [...chosen][0]! : null;
  const sentence = usersSentence(names.map((name) => {
    const k = byUser.get(name);
    const active = (k?.keys ?? []).filter((x) => x.Status === "Active");
    return { name, active: active.length, usedToday: active.some((x) => { const u = k?.used.get(x.AccessKeyId)?.LastUsedDate; return u !== undefined && sameDay(u, now); }) };
  }));
  const columns: DataColumn<Row>[] = [
    { id: "name", header: "User name", size: 260, min: 160, accessorFn: (r) => r.name.toLowerCase(), cell: (c) => <To kind="users" name={c.row.original.name} /> },
    { id: "key", header: "Access key ID", size: 380, min: 220, accessorFn: (r) => r.keys?.keys[0]?.AccessKeyId ?? "", meta: { mono: true }, cell: (c) => { const k = c.row.original.keys?.keys[0]; return !c.row.original.keys ? <span className="text-muted-foreground">…</span> : k ? <span className="font-mono text-[13px]">{k.Status} - {k.AccessKeyId}{(c.row.original.keys.keys.length > 1) && <span className="text-muted-foreground"> +{c.row.original.keys.keys.length - 1}</span>}</span> : <span className="text-muted-foreground">-</span>; } },
    { id: "age", header: "Access key age", size: 190, accessorFn: (r) => r.keys?.keys[0]?.CreateDate ?? 0, cell: (c) => { const k = c.row.original.keys?.keys[0]; return !c.row.original.keys ? <span className="text-muted-foreground">…</span> : k ? daysSince(k.CreateDate, now) : "None"; } },
    { id: "used", header: "Access key last used", size: 220, accessorFn: (r) => { const k = r.keys?.keys[0]; return (k && r.keys?.used.get(k.AccessKeyId)?.LastUsedDate) || 0; }, cell: (c) => { const r = c.row.original; const k = r.keys?.keys[0]; if (!r.keys) return <span className="text-muted-foreground">…</span>; if (!k) return <span className="text-muted-foreground">-</span>; const u = r.keys.used.get(k.AccessKeyId)?.LastUsedDate; return u === undefined ? <span title="IAM has no record of a call signed with this key">Never</span> : daysSince(u, now); } },
  ];
  const none = names.length === 0 && first !== undefined;
  return (
    <>
      <PageHeader
        title="Users"
        crumbs={listCrumbs(KINDS.users)}
        sub="An IAM user is an identity within your account that has specific permissions for a single person or application."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            {!none && (
              <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={chosenName === null} title={chosenName === null ? "Choose one user to delete" : undefined} onClick={() => setAsking(true)} data-testid="action-delete">
                <TrashIcon /> Delete
              </Button>
            )}
            <Button onClick={create} data-testid="create-open"><PlusIcon /> Create user</Button>
          </>
        }
      />
      {gone && <GoneBanner kind="User" name={gone} back={() => toList()} />}
      {created && <CreatedBanner kind="User" noun="user" kindId="users" name={created} returned="CreateUser answered, and the user is in the list. It has no access key until you make one." back={() => toList()} />}
      {list.isError && !first ? (
        <ListError what="The users could not be listed" error={list.error} retry={refresh} retrying={list.isFetching} />
      ) : !first ? (
        <ListLoading />
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={UserIcon} title="No users" action={<Button variant="outline" className="bg-card" onClick={create}><PlusIcon /> Create user</Button>}>
            There are no users in this account. Requests use the account root until you create one.
          </EmptyState>
        </div>
      ) : (
        <>
          <Say text={sentence} names={names} bold={[/^\d[\d,]* users?/]} className="mb-6" testId="list-sentence" />
          <SelectableTable
            aria-label="Users"
            data={rows}
            columns={columns}
            rowId={(r) => r.name}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="user"
            toolbar={<SearchBox value={typed} onChange={setTyped} noun="users" />}
            selectedLabel={(c) => `${c} selected`}
            maxHeight={680}
            empty={`No user's name contains “${typed}”.`}
          />
          <PageLine shown={names.length} noun="user" more={Boolean(list.hasNextPage)} loading={list.isFetchingNextPage} onMore={() => void list.fetchNextPage()} />
          {!ready && <p className="sr-only" aria-live="polite">Reading the access keys</p>}
        </>
      )}
      {chosenName && (
        <DeleteEntity kind="users" name={chosenName} open={asking} onOpenChange={setAsking} onDeleted={() => { setAsking(false); setChosen(new Set()); toList({ gone: chosenName }); }} />
      )}
    </>
  );
}
