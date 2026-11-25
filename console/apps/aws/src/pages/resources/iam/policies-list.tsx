// IAM's policies (iam__policies-list, policies-none): the customer managed policies, or the AWS managed ones that something has attached, with
// what each is attached to and a sentence of it. A policy is chosen with its radio; Actions attaches or deletes it. `?create=yes` is Create policy.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, FileTextIcon, PlusIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { POLICIES, useEach, useRead, useRefresh } from "./api";
import { AttachTo } from "./attach-to";
import { DeleteEntity } from "./delete-entity";
import { CreatedBanner, GoneBanner, ListError, ListLoading, PageLine, SearchBox, useToList } from "./list-parts";
import { managedKind } from "./model";
import { KINDS, listCrumbs, Note, Say, Segmented, To } from "./parts";
import { PolicyCreate } from "./policy-create";
import { policiesSentence } from "./sentences";

type Listed = { PolicyName: string; Arn: string; AttachmentCount?: number; Description?: string };
type Entities = { PolicyUsers?: { UserName: string }[]; PolicyRoles?: { RoleName: string }[]; PolicyGroups?: { GroupName: string }[] };

export function PoliciesList() {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string; do?: string; name?: string };
  if (search.create === "yes") return <PolicyCreate />;
  return <Policies gone={search.gone} created={search.do === "created" ? search.name : undefined} />;
}

function Policies({ gone, created }: { gone?: string; created?: string }) {
  const navigate = useNavigate();
  const toList = useToList("policies");
  const refreshAll = useRefresh();
  const [asked, setScope] = useUrlParam("scope");
  const scope: "Local" | "AWS" | "All" = asked === "aws" ? "AWS" : asked === "all" ? "All" : "Local";
  const read = useRead<{ Policies: Listed[]; IsTruncated?: boolean }>(POLICIES, "ListPolicies", { Scope: scope, MaxItems: 1000 });
  const all = useMemo(() => read.data?.Policies ?? [], [read.data]);
  const entities = useEach<Entities>(POLICIES, "ListEntitiesForPolicy", all.map((p) => ({ PolicyArn: p.Arn })));
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<string | null>(null);
  const [dialog, setDialog] = useState<"delete" | "attach" | null>(null);
  const rows = all.filter((p) => p.PolicyName.toLowerCase().includes(typed.trim().toLowerCase()));
  const pick = all.find((p) => p.Arn === chosen) ?? null;
  const facts = all.map((p, i) => ({ name: p.PolicyName, attached: [...(entities[i]?.data?.PolicyRoles ?? []).map((r) => ({ kind: "role" as const, name: r.RoleName })), ...(entities[i]?.data?.PolicyUsers ?? []).map((u) => ({ kind: "user" as const, name: u.UserName })), ...(entities[i]?.data?.PolicyGroups ?? []).map((g) => ({ kind: "group" as const, name: g.GroupName }))] }));
  const { refetch } = read;
  const refresh = useCallback(() => {
    void refetch();
    refreshAll();
  }, [refetch, refreshAll]);
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "policies" }, search: { create: "yes" } });
  const columns: DataColumn<Listed>[] = [
    { id: "choose", size: 44, enableSorting: false, header: () => <span className="sr-only">Choose</span>, cell: (c) => <RadioGroupItem value={c.row.original.Arn} aria-label={`Choose policy ${c.row.original.PolicyName}`} data-testid={`choose-${c.row.original.PolicyName}`} /> },
    { id: "name", header: "Policy name", size: 300, min: 190, accessorFn: (r) => r.PolicyName.toLowerCase(), cell: (c) => <To kind="policies" name={c.row.original.Arn} label={c.row.original.PolicyName} /> },
    { id: "type", header: "Type", size: 180, accessorFn: (r) => managedKind(r.Arn), cell: (c) => managedKind(c.row.original.Arn) },
    { id: "attached", header: "Attached entities", size: 190, accessorFn: (r) => r.AttachmentCount ?? 0, cell: (c) => c.row.original.AttachmentCount ?? 0 },
    { id: "description", header: "Description", accessorFn: (r) => r.Description ?? "", cell: (c) => c.row.original.Description || <span className="text-muted-foreground">–</span> },
  ];
  const none = read.isSuccess && all.length === 0;
  const noun = scope === "Local" ? "customer managed" : scope === "AWS" ? "AWS managed" : "";
  return (
    <>
      <PageHeader
        title="Policies"
        crumbs={listCrumbs(KINDS.policies)}
        sub="A policy is an entity that, when attached to an identity or resource, defines their permissions."
        refresh={<RefreshControl updatedAt={read.dataUpdatedAt} fetching={read.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            {!none && <DropdownMenu>
              <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="actions-menu">Actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-52">
                <DropdownMenuItem disabled={!pick} onSelect={() => setDialog("attach")} data-testid="action-attach">Attach to a user, group or role</DropdownMenuItem>
                <DropdownMenuItem disabled={!pick || managedKind(pick.Arn) === "AWS managed"} onSelect={() => setDialog("delete")} data-testid="action-delete">Delete</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>}
            <Button onClick={create} data-testid="create-open"><PlusIcon /> Create policy</Button>
          </>
        }
      />
      {gone && <GoneBanner kind="Policy" name={gone} back={() => toList()} />}
      {created && <CreatedBanner kind="Policy" noun="policy" kindId="policies" name={created} returned="CreatePolicy answered, and the policy starts at version v1, its default. Attach it to a user, group or role from Actions." back={() => toList()} />}
      {read.isError && !read.data ? (
        <ListError what="The policies could not be listed" error={read.error} retry={refresh} retrying={read.isFetching} />
      ) : !read.data ? (
        <ListLoading />
      ) : (
        <>
          {!none && <Say text={policiesSentence(facts, scope === "AWS" ? "AWS" : "Local")} names={all.map((p) => p.PolicyName)} bold={[/^\d[\d,]* (customer|AWS) managed polic(y|ies)/]} className="mb-6" testId="list-sentence" />}
          <div className="mb-3 flex flex-wrap items-center gap-3">
            <div className="min-w-0 flex-1">{!none && <SearchBox value={typed} onChange={setTyped} noun="policies" />}</div>
            <Segmented value={scope === "All" ? "" : scope === "AWS" ? "aws" : "local"} onChange={(v) => setScope(v === "aws" ? "aws" : v === "local" ? undefined : "all")} label="Policy type" options={[{ value: "aws", label: "AWS managed" }, { value: "local", label: "Customer managed" }]} testId="policy-scope" />
          </div>
          {none ? (
            <div data-testid="list-empty">
              <EmptyState icon={FileTextIcon} title={noun ? `No ${noun} policies` : "No policies"} action={<Button variant="outline" className="bg-card" onClick={create}><PlusIcon /> Create policy</Button>}>
                {scope === "AWS" ? "IAM lists an AWS managed policy once something has it attached. Attach one by its ARN and it appears here." : "Create a policy to define permissions you can attach to users, groups and roles."}
              </EmptyState>
            </div>
          ) : (
            <>
              <RadioGroup value={chosen ?? ""} onValueChange={setChosen} aria-label="Choose a policy" className="block">
                <DataTable aria-label="Policies" data={rows} columns={columns} rowId={(r) => r.Arn} maxHeight={680} rowClassName={(r) => (r.Arn === chosen ? "[&>td]:bg-pick/50" : undefined)} onRowClick={(r) => setChosen(r.Arn)} empty={`No policy's name contains “${typed}”.`} />
              </RadioGroup>
              {scope !== "Local" && <Note className="mt-3" testId="aws-note">IAM lists an AWS managed policy once something has it attached; doze-aws makes the others from their names when you attach them by ARN, so they are not listed before then.</Note>}
              <PageLine shown={all.length} noun="policy" many="policies" more={Boolean(read.data.IsTruncated)} loading={false} onMore={() => undefined} />
            </>
          )}
        </>
      )}
      {pick && <DeleteEntity kind="policies" name={pick.Arn} label={pick.PolicyName} open={dialog === "delete"} onOpenChange={(o) => !o && setDialog(null)} onDeleted={() => { setDialog(null); setChosen(null); toList({ gone: pick.PolicyName }); }} />}
      {pick && <AttachTo arn={pick.Arn} label={pick.PolicyName} open={dialog === "attach"} onOpenChange={(o) => !o && setDialog(null)} />}
    </>
  );
}
