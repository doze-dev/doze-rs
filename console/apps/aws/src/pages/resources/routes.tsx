// The Resources space's addresses, read and resolved: `/resources/<service>` opens its first kind, `/resources/<service>/<x>` is
// a kind's list or (from Live) a resource whose kind the call's input member says, `/resources/<service>/<kind>/<name>` is one.
import { getRouteApi, Link, Navigate } from "@tanstack/react-router";
import { LayoutGridIcon } from "lucide-react";
import { lazy, Suspense } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useCatalogue } from "../../api/resources";
import { ResourceDetailPage } from "./detail-page";
import { HINT } from "./index-page";
import { ResourceListPage } from "./list-page";
import { resolve } from "./model";
import { type OwnProps, ownOf, serviceOf } from "./deep/registry";
import "./deep/pages";

// An execution and a log stream are pages of their own, in a chunk of their own.
const DeepRoute = lazy(() => import("./deep/routes").then((m) => ({ default: m.DeepRoute })));
/** (The same test as `./deep/routes`, here so the chunk is fetched only when it is needed.) */
const isDeepRoute = (service: string, kind: string) => (service === "stepfunctions" && kind === "executions") || (service === "logs" && kind === "streams");

function NotDescribed({ service, note }: { service: string; note: string | null }) {
  return (
    <>
      <PageHeader title={service} crumbs={[{ label: "Resources", to: "/resources" }]} />
      <div data-testid="not-described">
        <EmptyState
          icon={LayoutGridIcon}
          title="Not described yet"
          action={
            <Button asChild variant="outline" className="bg-card">
              <Link to="/resources">All resources</Link>
            </Button>
          }
        >
          {note ?? "The console does not list this service's resources yet."}
        </EmptyState>
      </div>
    </>
  );
}

/** What the catalogue says of the address, or the page that says why there is nothing to show. */
function useResolved(service: string, second: string | null, member?: string) {
  const catalogue = useCatalogue();
  if (catalogue.isError && !catalogue.data) return { page: <ErrorBlock what="The resources could not be read" error={catalogue.error} hint={HINT} onRetry={() => void catalogue.refetch()} retrying={catalogue.isFetching} /> };
  if (!catalogue.data) return { page: <Skeleton className="h-80 rounded-[var(--radius-card)]" /> };
  const s = catalogue.data.services.find((x) => x.name === service);
  if (!s || !s.described) return { page: <NotDescribed service={s?.title ?? service} note={s?.note ?? null} /> };
  if (second === null) return { kind: s.kinds[0]?.id ?? "", name: null };
  return resolve(catalogue.data, service, second, member) ?? { page: <NotDescribed service={s.title} note={s.note} /> };
}

const serviceRoute = getRouteApi("/resources/$service");
const secondRoute = getRouteApi("/resources/$service/$second");
const oneRoute = getRouteApi("/resources/$service/$kind/$name");

export function ServicePage() {
  const { service } = serviceRoute.useParams();
  const own = serviceOf(service);
  const r = useResolved(service, null);
  if (own) return <Own Page={own as never} props={{ service, kind: "", name: null, search: {} }} />;
  if ("page" in r) return r.page;
  return <Navigate to="/resources/$service/$second" params={{ service, second: r.kind }} search={{}} replace />;
}

/** The page a service draws itself for this address, when its kind registered one (`registerOwn`). */
const ownPage = (service: string, kind: string, name: string | null, search: OwnProps["search"]) => {
  const own = ownOf(service, kind);
  return (search.do && own?.actions?.[search.do]) || (name === null ? own?.list : own?.one);
};

function Own({ Page, props }: { Page: NonNullable<ReturnType<typeof ownPage>>; props: OwnProps }) {
  return (
    <Suspense fallback={<Skeleton className="h-80 rounded-[var(--radius-card)]" />}>
      <Page {...props} />
    </Suspense>
  );
}

const ownKey = (p: OwnProps) => [p.service, p.kind, p.name ?? "", p.search.do ?? "", p.search.key ?? "", p.search.dialog ?? ""].join("/");

export function SecondPage() {
  const { service, second } = secondRoute.useParams();
  const { m, all, ...more } = secondRoute.useSearch();
  const r = useResolved(service, second, m);
  if ("page" in r) return r.page;
  const props: OwnProps = { service, kind: r.kind, name: r.name, search: more };
  const Page = ownPage(service, r.kind, r.name, more);
  if (Page) return <Own key={ownKey(props)} Page={Page} props={props} />;
  if (r.name === null) return <ResourceListPage key={`${service}/${r.kind}`} service={service} kind={r.kind} all={all === "yes"} />;
  return <ResourceDetailPage key={`${service}/${r.kind}/${r.name}`} service={service} kind={r.kind} name={r.name} />;
}

export function OnePage() {
  const { service, kind, name } = oneRoute.useParams();
  const { tab, group, ...more } = oneRoute.useSearch();
  const props: OwnProps = { service, kind, name, search: { tab, ...more } };
  const Page = ownPage(service, kind, name, props.search);
  if (Page) return <Own key={ownKey(props)} Page={Page} props={props} />;
  if (isDeepRoute(service, kind)) {
    return (
      <Suspense fallback={<Skeleton className="h-80 rounded-[var(--radius-card)]" />}>
        <DeepRoute key={`${service}/${kind}/${name}`} service={service} kind={kind} name={name} group={group} />
      </Suspense>
    );
  }
  return <ResourceDetailPage key={`${service}/${kind}/${name}`} service={service} kind={kind} name={name} tab={tab} />;
}
