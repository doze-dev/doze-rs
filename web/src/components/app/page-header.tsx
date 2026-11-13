import { Link } from "@tanstack/react-router";
import { ChevronRightIcon } from "lucide-react";
import { Fragment } from "react";
import { cn } from "@/lib/utils";

export type Crumb = { label: string; to: string };

/** The head of every page: the ancestors only as a breadcrumb (never the page itself, never a filter or a mode), the page's
 * name once as the h1, a static sentence, the page's actions, and at the right of the top row the RefreshControl. */
export function PageHeader({ title, sub, crumbs = [], actions, refresh, mono = false }: { title: string; sub?: React.ReactNode; crumbs?: Crumb[]; actions?: React.ReactNode; refresh?: React.ReactNode; mono?: boolean }) {
  return (
    <header className="mb-8" data-slot="page-header">
      <div className="flex min-h-9 flex-wrap items-center justify-between gap-x-6 gap-y-2">
        <nav aria-label="Breadcrumb" className="min-w-0">
          {crumbs.length > 0 && (
            <ol className="flex flex-wrap items-center gap-2 text-[13px] text-muted-foreground">
              {crumbs.map((c, i) => (
                <Fragment key={c.to}>
                  {i > 0 && <ChevronRightIcon aria-hidden className="size-3.5 text-input" />}
                  <li>
                    <Link to={c.to} className="rounded-sm hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                      {c.label}
                    </Link>
                  </li>
                </Fragment>
              ))}
            </ol>
          )}
        </nav>
        {refresh}
      </div>
      <div className="mt-5 flex flex-wrap items-start justify-between gap-x-8 gap-y-4">
        <div className="min-w-0 flex-1 basis-[28rem]">
          <h1 className={cn("text-[34px] leading-[42px] font-light tracking-[-0.03em] break-words text-bright", mono && "font-mono text-[27px] leading-[34px] font-medium tracking-[-0.01em]")}>{title}</h1>
          {sub && <p className="mt-2 max-w-[46rem] text-[14.5px] leading-[22px] text-muted-foreground">{sub}</p>}
        </div>
        {actions && <div className="flex flex-none flex-wrap items-center gap-2.5 pt-1">{actions}</div>}
      </div>
    </header>
  );
}
