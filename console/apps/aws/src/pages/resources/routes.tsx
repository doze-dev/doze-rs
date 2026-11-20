// The Resources space's addresses, read and resolved: `/resources/<service>` opens its first kind, `/resources/<service>/<x>` is
// a kind's list or (from Live) a resource whose kind the call's input member says, `/resources/<service>/<kind>/<name>` is one.
import { getRouteApi, Link, Navigate } from "@tanstack/react-router";
import { LayoutGridIcon } from "lucide-react";
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
  if (!catalogue.data) return { page: <Skeleton className="h-80 rounded-[14px]" /> };
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
  const r = useResolved(service, null);
  if ("page" in r) return r.page;
  return <Navigate to="/resources/$service/$second" params={{ service, second: r.kind }} search={{}} replace />;
}

export function SecondPage() {
  const { service, second } = secondRoute.useParams();
  const { m, all } = secondRoute.useSearch();
  const r = useResolved(service, second, m);
  if ("page" in r) return r.page;
  if (r.name === null) return <ResourceListPage key={`${service}/${r.kind}`} service={service} kind={r.kind} all={all === "yes"} />;
  return <ResourceDetailPage key={`${service}/${r.kind}/${r.name}`} service={service} kind={r.kind} name={r.name} />;
}

export function OnePage() {
  const { service, kind, name } = oneRoute.useParams();
  const { tab } = oneRoute.useSearch();
  return <ResourceDetailPage key={`${service}/${kind}/${name}`} service={service} kind={kind} name={name} tab={tab} />;
}
