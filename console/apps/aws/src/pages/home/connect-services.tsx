import { useMemo } from "react";
import { Link } from "@tanstack/react-router";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { useServices } from "../../api/queries";
import type { About } from "../../api/types/About";
import { CardSkeleton } from "../card";
import { InvokeSheet, S3Sheet } from "./connect-sheets";
import { cn } from "@doze/ui/lib/utils";
import { type Drawer, type ServiceUrl, serviceUrls } from "./service-urls";

const link = "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";

/** By service: where each service answers, the variable an SDK reads for it, and what is special about its URLs. */
export function ByService({ a }: { a: About }) {
  const services = useServices();
  // The sheet that is open is in the address (`?sheet=s3`).
  const [asked, setAsked] = useUrlParam("sheet");
  const open: Drawer | null = asked === "invoke" || asked === "s3" ? asked : null;
  const setOpen = (d: Drawer | null) => setAsked(d);
  const rows = useMemo(() => (services.data ? serviceUrls(services.data.services, a.endpoint, a.account, a.region) : null), [services.data, a]);
  const columns = useMemo<DataColumn<ServiceUrl>[]>(
    () => [
      { id: "service", header: "Service", size: 210, min: 150, enableSorting: false, accessorFn: (r) => r.title, cell: (c) => <Link to="/resources/$service" params={{ service: c.row.original.name }} className={cn(link, "font-medium")} data-testid="service-link">{c.getValue<string>()}</Link> },
      { id: "address", header: "On IP and port", size: 220, min: 170, enableSorting: false, accessorKey: "address", meta: { mono: true }, cell: (c) => <span className="inline-flex items-center gap-1.5">{c.getValue<string>()}<CopyButton text={c.getValue<string>()} iconOnly variant="ghost" label="Copy the address" what="Address copied" /></span> },
      { id: "vars", header: "Variable", size: 330, min: 220, enableSorting: false, accessorFn: (r) => r.vars.join(" "), cell: (c) => <span className="flex flex-col">{c.row.original.vars.map((v) => <span key={v}>{v}</span>)}</span>, meta: { mono: true } },
      {
        id: "detail",
        header: "Detail",
        enableSorting: false,
        accessorFn: (r) => r.detail ?? "",
        cell: (c) => {
          const r = c.row.original;
          return r.detail ? (
            <span className="flex flex-wrap items-baseline gap-x-3">
              <span>{r.detail}</span>
              {r.more && (
                <button type="button" className={link} onClick={() => setAsked(r.more!.drawer)} data-testid={`more-${r.name}`}>
                  {r.more.label}
                </button>
              )}
            </span>
          ) : null;
        },
      },
    ],
    [setAsked],
  );
  return (
    <div className="grid gap-6">
      <Callout tone="note" lead="One endpoint is enough." testId="services-lede">
        Every service answers on the one address; what changes from one to the next is the shape of the URLs it hands back, noted under Detail. The variable names are the SDKs&apos; own: each sends that service&apos;s calls to the address you give it, and <span className="font-mono">AWS_ENDPOINT_URL</span> alone sends them all. A client built for another Region still works, because routing is by credential scope.
      </Callout>
      <div data-testid="connect-by-service">
        {services.isError && !rows ? (
          <ErrorBlock what="The services could not be loaded" error={services.error} hint="Check that doze-aws is still running, then try again." onRetry={() => void services.refetch()} retrying={services.isFetching} testId="by-service-error" />
        ) : rows ? (
          <DataTable aria-label="Where each service answers" data={rows} columns={columns} rowId={(r) => r.name} maxHeight={900} empty="This build serves no service." />
        ) : (
          <CardSkeleton rows={6} />
        )}
      </div>
      <InvokeSheet open={open === "invoke"} onOpenChange={(o) => !o && setOpen(null)} a={a} />
      <S3Sheet open={open === "s3"} onOpenChange={(o) => !o && setOpen(null)} a={a} />
    </div>
  );
}
