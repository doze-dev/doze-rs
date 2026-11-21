import type { Crumb } from "@doze/ui/components/app/page-header";
import { useCatalogue } from "../api/resources";

/** The breadcrumb a service's pages begin with: `Resources`, the group it is listed under (`Observability`), the service (`CloudWatch Logs`).
 * Read from the catalogue, which makes no call and is kept, so a page that has read nothing else still knows where it is. */
export function useServiceCrumbs(service: string): Crumb[] {
  const s = useCatalogue().data?.services.find((x) => x.name === service);
  return [{ label: "Resources", to: "/resources" }, ...(s ? [{ label: s.group, to: "/resources" }, { label: s.title, to: `/resources/${service}` }] : [])];
}
