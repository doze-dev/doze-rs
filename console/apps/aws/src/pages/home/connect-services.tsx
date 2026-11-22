import { useMemo, useState } from "react";
import { InfoIcon } from "lucide-react";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { useServices } from "../../api/queries";
import type { About } from "../../api/types/About";
import { CardSkeleton } from "../card";
import { InvokeSheet, S3Sheet } from "./connect-sheets";
import { type Drawer, type ServiceUrl, serviceUrls } from "./service-urls";

const link = "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";

/** By service: where each service answers, the variable an SDK reads for it, and what is special about its URLs. */
export function ByService({ a }: { a: About }) {
  const services = useServices();
  const [open, setOpen] = useState<Drawer | null>(null);
  const rows = useMemo(() => (services.data ? serviceUrls(services.data.services, a.endpoint, a.account, a.region) : null), [services.data, a]);
  const columns = useMemo<DataColumn<ServiceUrl>[]>(
    () => [
      { id: "service", header: "Service", size: 210, min: 150, enableSorting: false, accessorFn: (r) => r.title, cell: (c) => <span className="font-medium text-bright">{c.getValue<string>()}</span> },
      { id: "address", header: "On IP and port", size: 220, min: 170, enableSorting: false, accessorKey: "address", meta: { mono: true }, cell: (c) => <span className="inline-flex items-center gap-1.5">{c.getValue<string>()}<CopyButton text={c.getValue<string>()} iconOnly variant="ghost" label="Copy the address" what="Address copied" /></span> },
      { id: "vars", header: "Variable", size: 330, min: 220, enableSorting: false, accessorFn: (r) => r.vars.join(" "), cell: (c) => <span className="flex flex-col">{c.row.original.vars.map((v) => <span key={v} className="inline-flex items-center gap-1.5">{v}<CopyButton text={v} iconOnly variant="ghost" label="Copy the variable" what="Variable copied" /></span>)}</span>, meta: { mono: true } },
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
                <button type="button" className={link} onClick={() => setOpen(r.more!.drawer)} data-testid={`more-${r.name}`}>
                  {r.more.label}
                </button>
              )}
            </span>
          ) : null;
        },
      },
    ],
    [],
  );
  return (
    <div className="grid gap-6">
      <Alert tone="info" title="One endpoint is enough." icon={InfoIcon} testId="services-lede">
         Every service answers on the one address; what changes from one to the next is the shape of the URLs it hands back, noted under Detail. The variable names are the SDKs&apos; own: each sends that service&apos;s calls to the address you give it, and <span className="font-mono">AWS_ENDPOINT_URL</span> alone sends them all. A client built for another Region still works, because routing is by credential scope.
      </Alert>
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
