// A REST API's Resources (apigateway__rest-resources): the tree of resources with the methods under each, the methods of the one chosen
// as a table, Create resource and Create method, and the API's Recent calls.
import { FolderIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { cn } from "@doze/ui/lib/utils";
import { useNavigate } from "@tanstack/react-router";
import type { CallRecord } from "../../../api/types/CallRecord";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { RecentCalls } from "../../../kit";
import { HINT } from "../index-page";
import { useDeployments, useResources, useStages } from "./api";
import { MethodDelete, ResourceDelete } from "./part-deletes";
import { ApiChips, ApiLink, Caption, Said } from "./parts";
import type { RestMethod } from "./types";
import { REST_SECTIONS } from "./sections";
import { flattenTree, integrationKind, methodsOf, restSentence } from "./words";

export function RestResources({ id, name, selected, calls }: { id: string; name: string; selected: string | undefined; calls: CallRecord[] }) {
  const navigate = useNavigate();
  const tree = useResources(id);
  const stages = useStages(id);
  const deployments = useDeployments(id);
  const resources = useMemo(() => tree.data?.item ?? [], [tree.data]);
  const rows = useMemo(() => flattenTree(resources), [resources]);
  const chosen = resources.find((r) => r.id === selected) ?? resources.find((r) => Object.keys(r.resourceMethods ?? {}).length > 0) ?? resources.find((r) => !r.parentId);
  const methods = chosen ? methodsOf(chosen) : [];
  const [dropResource, setDropResource] = useState(false);
  const [dropMethod, setDropMethod] = useState<RestMethod | null>(null);
  const create = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "restapis", name: id }, search: search as never });
  const columns: DataColumn<RestMethod>[] = [
    { id: "method", header: "Method type", size: 170, accessorFn: (m) => m.httpMethod, cell: (c) => <ApiLink kind="restapis" id={id} search={{ do: "method", key: chosen?.id, sel: c.row.original.httpMethod }} className="font-mono text-[13.5px] font-medium" testId="method-link">{c.row.original.httpMethod}</ApiLink> },
    { id: "integration", header: "Integration type", size: 220, accessorFn: (m) => integrationKind(m.methodIntegration) },
    { id: "authorization", header: "Authorization type", size: 220, accessorFn: (m) => m.authorizationType ?? "NONE", cell: (c) => ({ NONE: "None", AWS_IAM: "IAM", CUSTOM: "Lambda authorizer", COGNITO_USER_POOLS: "Cognito user pool" })[c.row.original.authorizationType ?? "NONE"] ?? c.row.original.authorizationType },
    { id: "key", header: "API key requirement", accessorFn: (m) => (m.apiKeyRequired ? "Required" : "Not required") },
    { id: "delete", header: "", size: 60, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete method ${c.row.original.httpMethod} ${chosen?.path ?? ""}`} onClick={() => setDropMethod(c.row.original)} data-testid={`delete-method-${c.row.original.httpMethod}`}><Trash2Icon /></Button> },
  ];
  if (tree.isError) return <ErrorBlock what="The resources could not be read" error={tree.error} hint={HINT} onRetry={() => void tree.refetch()} retrying={tree.isFetching} testId="resources-error" />;
  return (
    <>
      <Said text={restSentence(name, resources, stages.data?.item ?? [], deployments.data?.item ?? [])} names={[name, ...(stages.data?.item ?? []).map((s) => s.stageName), ...(deployments.data?.item ?? []).map((d) => d.id)]} />
      <ApiChips kind="restapis" id={id} name={name} sections={REST_SECTIONS} current="resources" />
      <Caption title="Resources" action={<div className="flex flex-wrap gap-2"><Button variant="outline" className="bg-card text-bad hover:text-bad" disabled={!chosen?.parentId} title={chosen && !chosen.parentId ? "The root resource cannot be deleted." : undefined} onClick={() => setDropResource(true)} data-testid="delete-resource"><Trash2Icon /> Delete resource</Button><Button variant="outline" className="bg-card" onClick={() => create({ do: "create-resource", key: chosen?.id })} data-testid="create-resource"><PlusIcon /> Create resource</Button></div>} />
      <section className="overflow-hidden rounded-[var(--radius-card)] border bg-card" aria-label="Resources" data-testid="resource-tree">
        <ul>
          {rows.map(({ resource: r, depth }) => {
            const on = r.id === chosen?.id;
            return (
              <li key={r.id} className="border-b last:border-b-0" data-testid="resource-node" data-path={r.path}>
                <ApiLink kind="restapis" id={id} search={{ key: r.id }} className={cn("flex h-14 items-center gap-3 border-l-[3px] pr-5 font-mono text-[14.5px] font-medium text-bright no-underline hover:no-underline", on ? "border-primary bg-pick/60" : "border-transparent hover:bg-accent/50")} testId={`resource-${r.path}`}>
                  <span style={{ paddingLeft: `${16 + depth * 32}px` }} className="flex items-center gap-3"><FolderIcon aria-hidden className="size-[18px] text-muted-foreground" />{r.path}</span>
                </ApiLink>
                {methodsOf(r).map((m) => (
                  <div key={m.httpMethod} className="flex h-14 items-center border-t" style={{ paddingLeft: `${16 + (depth + 1) * 32 + 30}px` }}>
                    <ApiLink kind="restapis" id={id} search={{ do: "method", key: r.id, sel: m.httpMethod }} className="rounded-full border bg-well px-3 py-1 font-mono text-[12.5px] text-bright no-underline hover:bg-accent hover:no-underline" testId={`pill-${r.path}-${m.httpMethod}`}>{m.httpMethod}</ApiLink>
                  </div>
                ))}
              </li>
            );
          })}
        </ul>
      </section>
      <p className="mt-4 mb-10 text-[13.5px] text-muted-foreground">The resources form a tree; the methods of a resource sit under it.</p>
      <Caption title={`Methods of ${chosen?.path ?? "/"}`} action={<Button variant="outline" className="bg-card" disabled={!chosen} onClick={() => create({ do: "create-method", key: chosen?.id })} data-testid="create-method"><PlusIcon /> Create method</Button>} />
      <DataTable<RestMethod> aria-label={`Methods of ${chosen?.path ?? "/"}`} data={methods} columns={columns} rowId={(m) => m.httpMethod} empty={`${chosen?.path ?? "/"} has no method yet. Create one to connect it to a backend.`} />
      <RecentCalls className="mt-12" calls={calls} noun="API" title="Recent calls" scope={name} />
      {chosen && <ResourceDelete api={id} resources={resources} resource={chosen} open={dropResource} onOpenChange={setDropResource} onDone={() => { setDropResource(false); create({}); }} />}
      {chosen && dropMethod && <MethodDelete api={id} resource={chosen} method={dropMethod} open onOpenChange={() => setDropMethod(null)} onDone={() => setDropMethod(null)} />}
    </>
  );
}
