// The IAM Dashboard (iam__dashboard): who can do what in this account. The counts GetAccountSummary gives, each a way to its list, the identity
// providers the account holds, and the account's ID and alias with the form that changes the alias (`?edit=alias`).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { FileTextIcon, FingerprintIcon, PencilIcon, UserRoundCogIcon, UserRoundIcon, UsersIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { usePrincipals } from "../../../api/spaces";
import { HINT } from "../index-page";
import { USERS, useIamCalls, useRead, useRefresh } from "./api";
import { AliasDialog } from "./alias-edit";
import { count, hhmm } from "./model";
import { crumbs, Say, Summary } from "./parts";
import { clientName } from "../../../people";

type Totals = { SummaryMap: { Users: number; Groups: number; Roles: number; Policies: number } };

export function Dashboard() {
  const search = useSearch({ strict: false }) as { edit?: string };
  const navigate = useNavigate();
  return (
    <>
      <DashboardPage />
      <AliasDialog open={search.edit === "alias"} onOpenChange={(o) => !o && void navigate({ to: "/resources/$service", params: { service: "iam" }, search: {} as never })} />
    </>
  );
}

function DashboardPage() {
  const navigate = useNavigate();
  const refreshAll = useRefresh();
  const about = useAbout().data;
  const summary = useRead<Totals>(USERS, "GetAccountSummary", {});
  const aliases = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {});
  const oidc = useRead<{ OpenIDConnectProviderList: unknown[] }>(USERS, "ListOpenIDConnectProviders", {});
  const saml = useRead<{ SAMLProviderList: unknown[] }>(USERS, "ListSAMLProviders", {});
  const calls = useIamCalls();
  const principals = usePrincipals("all");
  const account = about?.account ?? "";
  const alias = aliases.data?.AccountAliases[0] ?? null;
  const s = summary.data?.SummaryMap;
  const refresh = () => {
    void summary.refetch();
    refreshAll();
  };
  const users = new Map((principals.data?.principals ?? []).filter((p) => p.kind === "user" && p.name).map((p) => [p.key, p.name as string]));
  const last = (calls.data?.calls ?? []).slice().reverse().find((c) => c.caller !== null && users.has(c.caller) && c.action !== null);
  const edit = () => void navigate({ to: "/resources/$service", params: { service: "iam" }, search: { edit: "alias" } as never });
  const lastWords = last ? ` The last call signed with an IAM user's key was ${last.action} by ${users.get(last.caller!)}${last.sdk ? ` (${clientName(last.sdk)})` : ""} at ${hhmm(last.at_ms / 1000)}.` : " No call in the Live record was signed with an IAM user's key.";
  const sentence = s ? `${alias ?? account} has ${count(s.Users, "user")}, ${count(s.Groups, "group")}, ${count(s.Roles, "role")} and ${count(s.Policies, "customer managed policy", "customer managed policies")}.${calls.isSuccess ? lastWords : ""}` : "";
  const cards = [
    { id: "users", label: "Users", icon: UserRoundIcon, n: s?.Users, words: "in this account" },
    { id: "groups", label: "User groups", icon: UsersIcon, n: s?.Groups, words: "in this account" },
    { id: "roles", label: "Roles", icon: UserRoundCogIcon, n: s?.Roles, words: "in this account" },
    { id: "policies", label: "Policies", icon: FileTextIcon, n: s?.Policies, words: "customer managed" },
  ];
  return (
    <>
      <PageHeader
        title="IAM Dashboard"
        crumbs={crumbs()}
        sub={<>Who can do what in this account. Identity and Access Management (IAM) keeps the users, groups, roles and policies of account <span className="font-mono">{account}</span>.</>}
        refresh={<RefreshControl updatedAt={summary.dataUpdatedAt} fetching={summary.isFetching} onRefresh={refresh} interval={false} />}
      />
      {summary.isError && !s ? (
        <ErrorBlock what="The account summary could not be read" error={summary.error} hint={HINT} onRetry={refresh} retrying={summary.isFetching} testId="dashboard-error" />
      ) : !s ? (
        <Skeleton className="h-64 rounded-[14px]" />
      ) : (
        <div className="grid gap-8">
          <Say text={sentence} names={[...(alias ? [alias] : []), ...(last ? [last.action!, users.get(last.caller!)!] : [])]} bold={[/\b\d+ (users?|groups?|roles?|customer managed polic(y|ies))\b/, /\b\d\d:\d\d\b/]} testId="dashboard-sentence" />
          <section className="grid gap-4" aria-label="Users, groups, roles and policies" data-testid="counts-card">
            <div><h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Users, groups, roles and policies</h2><p className="mt-1.5 text-[13.5px] leading-[21px] text-muted-foreground">The counts come from GetAccountSummary. Choose one to open its list.</p></div>
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {cards.map((c) => (
                <Link key={c.id} to="/resources/$service/$second" params={{ service: "iam", second: c.id }} search={{}} className="grid gap-1 rounded-xl border bg-card p-4 outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50" data-testid={`count-${c.id}`}>
                  <span className="flex items-center gap-2 text-[14.5px] font-medium text-bright"><c.icon aria-hidden className="size-4 text-muted-foreground" />{c.label}</span>
                  <span className="text-[13px] text-muted-foreground" data-testid={`count-${c.id}-n`}>{c.n ?? "–"} {c.words}</span>
                </Link>
              ))}
              <div className="grid gap-1 rounded-xl border bg-card p-4" data-testid="count-providers">
                <span className="flex items-center gap-2 text-[14.5px] font-medium text-bright"><FingerprintIcon aria-hidden className="size-4 text-muted-foreground" />Identity providers</span>
                <span className="text-[13px] text-muted-foreground">{oidc.isSuccess && saml.isSuccess ? `${oidc.data.OpenIDConnectProviderList.length} OpenID Connect and ${saml.data.SAMLProviderList.length} SAML in this account` : "reading…"}</span>
              </div>
            </div>
          </section>
          <Summary title="AWS Account" facts={[
            { label: "Account ID", value: <span className="font-mono">{account}</span> },
            { label: "Account Alias", value: <span className="grid gap-1.5"><span className="flex flex-wrap items-center gap-3">{alias ? <span className="font-mono">{alias}</span> : <span className="text-muted-foreground">None</span>}<Button type="button" variant="outline" size="sm" className="bg-card" onClick={edit} data-testid="alias-edit"><PencilIcon /> Edit</Button></span><span className="max-w-[44rem] text-[13px] leading-5 text-muted-foreground" data-testid="alias-note">An account can have only one alias. Saving a new alias deletes the old one first (DeleteAccountAlias), then makes the new one (CreateAccountAlias).</span></span> },
          ]} />
        </div>
      )}
    </>
  );
}
