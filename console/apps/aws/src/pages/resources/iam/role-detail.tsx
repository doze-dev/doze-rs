// A role's page (iam__role-detail, role-trust, role-policies-none): what the record says of its sessions and what it may do, the strip
// from who assumes it to what it reaches, the Summary, and the tabs Permissions, Trust relationships and Tags. `?view=trust` is Edit trust policy.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, PencilIcon, ShieldOffIcon } from "lucide-react";
import { lazy, Suspense, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { stringifySearch } from "@doze/ui/lib/search-params";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useAbout } from "../../../api/queries";
import { usePrincipals } from "../../../api/spaces";
import { useNow } from "../../card";
import { RecentCalls, ResourcePage, TagsTab, WiredStrip, type Wired } from "../../../kit";
import { assumedOf, ROLES, USERS, useIamCalls, useRead } from "./api";
import { DeleteEntity } from "./delete-entity";
import { AttachPolicies } from "./dialogs";
import { Entity } from "./entity";
import { useToList } from "./list-parts";
import { type AttachedPolicy, type Doc, docText, permissions, sessionWords, trustPrincipals, trustSentence } from "./model";
import { Card, fieldOf, Note, PolicyTable, Say, Summary } from "./parts";
import { InlinePolicy } from "./inline-policy";
import { docResources, usePolicyDocs } from "./policy-data";
import { usePolicyOptions } from "./policy-chooser";
import { roleSentence } from "./sentences";
import { FEDERATED_WORDS, TrustNote } from "./trust-note";

const TrustEdit = lazy(() => import("./role-trust-edit").then((m) => ({ default: m.RoleTrustEdit })));

export function RoleDetail({ name }: { service: string; kind: string; name: string }) {
  const search = useSearch({ strict: false }) as { view?: string };
  if (search.view === "trust") return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Entity kind="roles" name={name} render={(d) => <TrustEdit role={name} doc={fieldDoc(d)} />} /></Suspense>;
  return <Entity kind="roles" name={name} render={(d, t) => <RoleView d={d} {...t} />} />;
}

const fieldDoc = (d: { fields: readonly { label: string; value: string | null }[] }): string => d.fields.find((f) => f.label === "Trust policy")?.value ?? "{}";

function RoleView({ d, updatedAt, fetching, refresh }: { d: Parameters<Parameters<typeof Entity>[0]["render"]>[0]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const name = d.name;
  const search = useSearch({ strict: false }) as { done?: string };
  const about = useAbout().data;
  const toList = useToList("roles");
  const navigate = useNavigate();
  const [tab] = useUrlParam("tab");
  const [deleting, setDeleting] = useState(false);
  const [dialog, setDialog] = useState<"attach" | "inline" | null>(null);
  const attached = useRead<{ AttachedPolicies: AttachedPolicy[] }>(ROLES, "ListAttachedRolePolicies", { RoleName: name });
  const inline = useRead<{ PolicyNames: string[] }>(ROLES, "ListRolePolicies", { RoleName: name });
  const calls = useIamCalls();
  const now = useNow();
  const principals = usePrincipals("all");
  const options = usePolicyOptions();
  const policies = attached.data?.AttachedPolicies ?? [];
  const inlines = inline.data?.PolicyNames ?? [];
  const docs = usePolicyDocs(policies.map((p) => p.PolicyArn));
  const trust = fieldDoc(d);
  const keyNames = new Map((principals.data?.principals ?? []).filter((p) => p.kind === "user" && p.name).map((p) => [p.key, p.name as string]));
  const seen = assumedOf(calls.data?.calls ?? [], name, keyNames, now);
  const ready = attached.isSuccess && inline.isSuccess;
  const rows = [...policies.map((p) => ({ ...p, entities: options.options.find((o) => o.arn === p.PolicyArn)?.attached ?? null })), ...inlines.map((n) => ({ PolicyName: n, PolicyArn: `inline:${n}`, entities: null }))];
  // The one permission of a role with one customer managed policy that grants one thing.
  const grants = policies.length === 1 && inlines.length === 0 && docs.size === 1 ? permissions([...docs.values()][0]!) : [];
  const only = grants.length === 1 && !grants[0]!.actions.includes(",") ? { action: grants[0]!.actions, resource: grants[0]!.resource.replace("arn:aws:s3:::", "") } : null;
  const users = trustPrincipals(trust).filter((p) => p.kind === "user");
  // How much of the trust policy is federated (web identity or SAML): none, some, or all of its principals.
  const federated = trustPrincipals(trust).filter((p) => p.kind === "federated").length;
  const federation: "none" | "some" | "all" = federated === 0 ? "none" : federated === trustPrincipals(trust).length ? "all" : "some";
  const reaches = docResources(docs.values()).slice(0, 3);
  const wired: Wired[] = [
    ...users.slice(0, 3).map((p) => ({ label: "user", to: { service: "iam", kind: "users", name: p.label }, note: null })),
    ...policies.slice(0, 3).map((p) => ({ label: "policy", to: { service: "iam", kind: "policies", name: p.PolicyArn }, note: p.PolicyName })),
    ...reaches.map((r) => ({ label: "resource", to: { service: r.service, kind: r.kind, name: r.name }, note: r.label })),
  ];
  const nUsers = Math.min(users.length, 3);
  // The sentence is on every tab; the strip, the Summary and the calls are the Permissions tab's.
  const main = (tab ?? "permissions") === "permissions";
  const sentence = tab === "trust" ? trustSentence(name, trust, about?.account ?? "") : roleSentence({ name, assumed: seen.count, inLastHour: seen.lastHour, by: seen.by, policies: [...policies.map((p) => p.PolicyName), ...inlines], only });
  const max = Number(d.fields.find((f) => f.label === "Maximum session")?.value ?? 3600);
  const alias = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {}).data?.AccountAliases[0] ?? about?.account ?? "";
  // Written as the router writes an address, so an account ID of digits (or a role named by digits) is read back as the text it is.
  const link = `/resources/sts${stringifySearch({ account: alias, roleName: name })}`;
  const description = fieldOf(d, "Description");
  const created = fieldOf(d, "Created");
  const shown = { ...d, sentence: null, wired: [], verbs: [], badges: [{ text: "IAM role", tone: "neutral" as const }] };
  const addPermissions = (
    <DropdownMenu>
      <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="add-permissions">Add permissions <ChevronDownIcon /></Button></DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-52">
        <DropdownMenuItem onSelect={() => setDialog("attach")} data-testid="attach-policies">Attach policies</DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setDialog("inline")} data-testid="create-inline">Create inline policy</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
  const tabs = [
    {
      id: "permissions",
      label: "Permissions",
      render: () => (
        <div className="grid gap-6">
          {rows.length === 0 && ready ? (
            <EmptyTab icon={ShieldOffIcon} title="No permissions policies" action={addPermissions} testId="permissions-none">{name} has no policies attached, so it is allowed to do nothing.</EmptyTab>
          ) : (
            <Card title={`Permissions policies (${rows.length})`} lede="Policies define what a session of this role can do." actions={addPermissions} testId="permissions-card">
              <PolicyTable rows={rows} entities />
              <Note testId="session-note">Maximum session duration is the longest a session of this role can last: 1 hour by default, up to 12 hours. <span className="dim">AssumeRole refuses a DurationSeconds above this role&apos;s, which is {sessionWords(max)}.</span></Note>
            </Card>
          )}
          <Card title="Permissions boundary" testId="boundary-card" lede={fieldOf(d, "Permissions boundary") ? <>The boundary is <span className="font-mono text-bright">{fieldOf(d, "Permissions boundary")}</span>. A permissions boundary controls the maximum permissions this role can have.</> : "Not set. A permissions boundary controls the maximum permissions this role can have."} />
        </div>
      ),
    },
    {
      id: "trust",
      label: "Trust relationships",
      render: () => (
        <Card title="Trusted entities" lede="Entities that can assume this role under the conditions in the trust policy." actions={<Button type="button" variant="outline" className="bg-card" onClick={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "roles", name }, search: { view: "trust" } })} data-testid="edit-trust"><PencilIcon /> Edit trust policy</Button>} testId="trust-card">
          <CodeBlock text={`// the role's trust policy\n${JSON.stringify(parse(trust), null, 2)}`} wrap={false} testId="trust-json" json />
          <TrustNote federated={federation === "all"} extra={<>doze-aws stores the trust policy exactly as it was written and returns it unchanged.{federation === "some" ? ` For the federated principals: ${FEDERATED_WORDS}` : ""}</>} />
        </Card>
      ),
    },
    { id: "tags", label: "Tags", render: () => <TagsTab target={{ service: "iam", kind: "roles", name }} noun="tag" explain="Tags are key-value labels on the role." /> },
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={description ?? (created ? `Created ${created}.` : undefined)}
        banner={search.done === "trust" ? <ResultBanner title={`The trust policy of ${name} was updated.`} returned="UpdateAssumeRolePolicy answered, and the policy below is the one now in force." testId="trust-updated" /> : undefined}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={[{ label: "Delete role", onSelect: () => setDeleting(true) }]}
        overview={
          <>
            {ready && calls.isSuccess ? <Say text={sentence} names={[name, ...policies.map((p) => p.PolicyName), ...users.map((u) => u.label), ...seen.by, ...(only ? [only.resource] : []), ...(only ? [only.action] : [])]} bold={[/\b\d+ times?\b/, /\b(one|two|three|no) (principals?|actions?)\b/]} className="mb-6" testId="detail-sentence" /> : <Skeleton className="mb-6 h-6 w-2/3" />}
            {main && wired.length > 0 && <WiredStrip wired={wired} self={{ name, at: nUsers }} chipText={(w) => (w.label === "user" ? `${w.to.name} user` : w.label === "policy" ? (w.note ?? w.to.name) : (w.note ?? w.to.name))} chipHref={(w) => (w.label === "policy" ? `/resources/iam/policies/${encodeURIComponent(w.to.name)}` : undefined)} />}
            {main && <Summary facts={[
              { label: "Created", value: created },
              { label: "Role ARN", value: <span className="font-mono">{fieldOf(d, "ARN")}</span> },
              { label: "Maximum session duration", value: sessionWords(max) },
              { label: "Role link", value: <a href={link} className="font-mono text-link hover:underline" data-testid="role-link">{`${window.location.origin}${link}`}</a> },
            ]} />}
          </>
        }
        tabs={tabs}
        tab={tab ?? "permissions"}
        onTab={(id) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "roles", name }, search: id === "permissions" ? {} : { tab: id } })}
        details={false}
        testId="role-page"
      />
      {main && <div className="mt-12"><RecentCalls calls={d.calls} noun="role" title="Recent calls" /></div>}
      <AttachPolicies on="roles" name={name} attached={policies.map((p) => p.PolicyArn)} open={dialog === "attach"} onOpenChange={(o) => !o && setDialog(null)} />
      <InlinePolicy role={name} open={dialog === "inline"} onOpenChange={(o) => !o && setDialog(null)} />
      <DeleteEntity kind="roles" name={name} open={deleting} onOpenChange={setDeleting} onDeleted={() => { setDeleting(false); toList({ gone: name }); }} />
      {attached.isError && <Alert tone="bad" title="The role's policies could not be read" testId="policies-error">{String((attached.error as Error).message)}</Alert>}
    </>
  );
}

const parse = (doc: Doc): unknown => {
  try {
    return typeof doc === "string" ? JSON.parse(docText(doc)) : doc;
  } catch {
    return doc;
  }
};
