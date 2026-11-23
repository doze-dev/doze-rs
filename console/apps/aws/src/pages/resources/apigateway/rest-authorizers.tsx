// A REST API's Authorizers (apigateway__rest-authorizers, rest-authorizers-none): the authorizers with what they read and call, which
// methods use them, and Create authorizer. An authorizer is attached to a method on its method request.
import { useNavigate } from "@tanstack/react-router";
import { PlusIcon, ShieldCheckIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { PlainConfirm, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { REST, useAuthorizers, useResources, useWrite } from "./api";
import { ApiChips, Said } from "./parts";
import { REST_SECTIONS } from "./sections";
import type { Authorizer } from "./types";
import { count, lambdaName, list } from "./words";

const payload = (a: Authorizer) => ({ TOKEN: "Token", REQUEST: "Request", COGNITO_USER_POOLS: "Cognito" })[a.type] ?? a.type;
const source = (a: Authorizer) => (a.identitySource ?? "").split(",").map((s) => s.replace(/^method\.request\.(header|querystring|multivaluequerystring|multivalueheader)\./, "")).join(", ");

export function RestAuthorizers({ id, name }: { id: string; name: string }) {
  const navigate = useNavigate();
  const auths = useAuthorizers(id);
  const tree = useResources(id);
  const del = useWrite(REST, "DeleteAuthorizer");
  const [deleting, setDeleting] = useState<Authorizer | null>(null);
  const create = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "restapis", name: id }, search: { do: "create-authorizer" } });
  if (auths.isError) return <ErrorBlock what="The authorizers could not be read" error={auths.error} hint={HINT} onRetry={() => void auths.refetch()} retrying={auths.isFetching} testId="authorizers-error" />;
  if (!auths.data) return null;
  const all = auths.data.item ?? [];
  const methods = (tree.data?.item ?? []).flatMap((r) => Object.values(r.resourceMethods ?? {}).map((m) => ({ path: r.path, m })));
  const usedBy = (a: Authorizer) => methods.filter((x) => x.m.authorizerId === a.id).map((x) => `${x.m.httpMethod} ${x.path}`);
  const unused = all.filter((a) => usedBy(a).length === 0);
  const sentence = all.length === 0 ? "" : `${name} has ${all.length === 1 ? "one authorizer" : `${count(all.length, "authorizer")}`}, ${list(all.map((a) => a.name))}. ${unused.length === 0 ? `${all.length === 1 ? "It is" : "They are"} used by ${list(all.flatMap(usedBy))}.` : unused.length === all.length ? `No method uses ${all.length === 1 ? "it" : "them"} yet: choose ${all.length === 1 ? "it" : "one"} under Authorization on a method request.` : `${list(unused.map((a) => a.name))} ${unused.length === 1 ? "is" : "are"} used by no method yet.`}`;
  const columns: DataColumn<Authorizer>[] = [
    { id: "name", header: "Authorizer name", min: 200, accessorFn: (a) => a.name, cell: (c) => <span className="font-mono text-[13.5px] font-medium text-bright" data-testid="authorizer-name">{c.row.original.name}</span> },
    { id: "payload", header: "Lambda event payload", size: 190, accessorFn: payload },
    { id: "fn", header: "Lambda function", size: 190, accessorFn: (a) => lambdaName(a.authorizerUri) ?? a.providerARNs?.[0] ?? "", cell: (c) => <span className="font-mono text-[13px]">{lambdaName(c.row.original.authorizerUri) ?? c.row.original.providerARNs?.map((p) => p.split("/").pop()).join(", ") ?? "–"}</span> },
    { id: "source", header: "Identity source", size: 190, accessorFn: source },
    { id: "ttl", header: "TTL", size: 130, meta: { numeric: true }, accessorFn: (a) => a.authorizerResultTtlInSeconds ?? 0, cell: (c) => `${c.row.original.authorizerResultTtlInSeconds ?? 0} seconds` },
    { id: "delete", header: "", size: 60, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete authorizer ${c.row.original.name}`} onClick={() => setDeleting(c.row.original)} data-testid={`delete-${c.row.original.name}`}><Trash2Icon /></Button> },
  ];
  return (
    <>
      {all.length > 0 && <Said text={sentence} names={[name, ...all.map((a) => a.name)]} />}
      <ApiChips kind="restapis" id={id} name={name} sections={REST_SECTIONS} current="authorizers" />
      {all.length === 0 ? (
        <EmptyState icon={ShieldCheckIcon} title="No authorizers" action={<Button variant="outline" className="bg-card" onClick={create} data-testid="create-authorizer-empty"><PlusIcon /> Create authorizer</Button>}>An authorizer checks a caller&apos;s identity before a method runs.</EmptyState>
      ) : (
        <>
          <DataTable<Authorizer> aria-label="Authorizers" data={all} columns={columns} rowId={(a) => a.id} />
          <div className="mt-6 grid max-w-3xl gap-3">
            <RuleHelp>A <b className="font-semibold text-foreground">token</b> authorizer reads one header, such as a JWT or an OAuth token. A <b className="font-semibold text-foreground">request</b> authorizer reads headers, query strings, stage variables and context variables, and the developer guide recommends it over the token type.</RuleHelp>
            <RuleHelp>Whatever the type, the Lambda function returns an IAM policy and a principal identifier; without both, the call fails. A Cognito authorizer checks the token&apos;s claims; doze-aws does not check its signature, because no user pool is served here.</RuleHelp>
          </div>
        </>
      )}
      <PlainConfirm
        open={deleting !== null}
        onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }}
        title="Delete authorizer?"
        description={<>Delete <b className="font-mono font-semibold text-bright">{deleting?.name}</b>?{deleting && usedBy(deleting).length > 0 ? <> {list(usedBy(deleting))} still {usedBy(deleting).length === 1 ? "names" : "name"} it.</> : " No method uses it."}</>}
        call="DeleteAuthorizer"
        pending={del.isPending}
        error={del.error}
        onConfirm={() => deleting && del.mutate({ restApiId: id, authorizerId: deleting.id } as never, { onSuccess: () => setDeleting(null) })}
        testId="authorizer-delete"
      />
    </>
  );
}
