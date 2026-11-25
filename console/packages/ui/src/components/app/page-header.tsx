import { Link } from "@tanstack/react-router";
import { ChevronRightIcon } from "lucide-react";
import { Fragment, useEffect, useRef, useState } from "react";
import { cn } from "../../lib/utils";

export type Crumb = { label: string; to: string };

/** Whether the page has scrolled under the title row (a 1px mark above it has left the area below the top bar, 52px). */
function useStuck() {
  const sentinel = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const el = sentinel.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    // The top bar is 52 px: the sentinel is under it once it has left the area below the bar.
    const io = new IntersectionObserver(([e]) => setStuck(Boolean(e) && !e!.isIntersecting && e!.boundingClientRect.top < 53), { rootMargin: "-53px 0px 0px 0px" });
    io.observe(el);
    return () => io.disconnect();
  }, []);
  return [sentinel, stuck] as const;
}

/** The head of every page: the breadcrumb (the ancestors as links, then the page itself as the last crumb, plain text with
 * aria-current: the trail always says where you are, and never a filter or a mode), the page's name once as the h1, a static sentence, the page's actions, and the RefreshControl.
 *
 * One shape for every page, so no page has an empty band: the first row (the breadcrumb, with the RefreshControl at its right) exists
 * only on a page that has ancestors or a RefreshControl, and the page's actions are always on the h1's row, at the right. The
 * RefreshControl is the page's housekeeping and the actions are what the page is for: they are never one stack, which read as one
 * group of buttons. A page without a RefreshControl simply has nothing in that row.
 *
 * From 1280 px the title row (the name, its badges, the actions) stays under the top bar while the page scrolls, so the verbs of a long
 * page are never out of reach. Nothing is drawn twice and nothing changes size on the way: the row is sticky in place, and only the line
 * under it appears once the page is under it. The header element itself is `display: contents`, so the row is sticky in the page and not
 * in the few lines of the header. */
export function PageHeader({ title, sub, crumbs = [], actions, refresh, mono = false, titleExtra, belowCrumbs }: { title: string; sub?: React.ReactNode; crumbs?: Crumb[]; actions?: React.ReactNode; refresh?: React.ReactNode; mono?: boolean; /** Beside the h1: the badges of a resource (its type, its state). */ titleExtra?: React.ReactNode; /** Between the breadcrumb and the title: a wizard's steps. */ belowCrumbs?: React.ReactNode }) {
  const top = crumbs.length > 0 || Boolean(refresh);
  const side = actions;
  const [mark, stuck] = useStuck();
  return (
    <header className="contents" data-slot="page-header">
      {top && (
        <div className="mb-5 flex min-h-9 flex-wrap items-center justify-between gap-x-6 gap-y-2">
          {crumbs.length > 0 && <nav aria-label="Breadcrumb" className="min-w-0">
            <ol className="flex flex-wrap items-center gap-2 text-[13px] text-muted-foreground">
              {crumbs.map((c, i) => (
                <Fragment key={`${i}-${c.to}`}>
                  {i > 0 && <ChevronRightIcon aria-hidden className="size-3.5 text-input" />}
                  <li>
                    {i === crumbs.length - 1 && crumbs.length > 1 ? (
                      <span aria-current="page" className="text-foreground">
                        {c.label}
                      </span>
                    ) : (
                      <Link to={c.to} className="rounded-sm hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
                        {c.label}
                      </Link>
                    )}
                  </li>
                </Fragment>
              ))}
            </ol>
          </nav>}
          {refresh && <div className="ml-auto">{refresh}</div>}
        </div>
      )}
      {belowCrumbs && <div className="mb-6">{belowCrumbs}</div>}
      <div ref={mark} aria-hidden className="-mb-px h-px" />
      <div className={cn("-mt-2 flex flex-wrap items-start justify-between gap-x-8 gap-y-4 bg-background py-2 xl:sticky xl:top-[52px] xl:z-[15] -mx-6 px-6 lg:-mx-10 lg:px-10 xl:-mx-12 xl:px-12", sub ? "mb-0" : "mb-6", stuck ? "shadow-[0_1px_0_0_var(--border)]" : "shadow-[0_1px_0_0_transparent]")} data-slot="page-title-row" data-stuck={stuck || undefined}>
        <div className="min-w-0 flex-1 basis-[14rem]">
          {titleExtra ? (
            <div className="flex flex-wrap items-center gap-x-3.5 gap-y-2">
              <h1 className={cn("text-[34px] leading-[42px] font-light tracking-[-0.03em] break-words text-bright", mono && "font-mono text-[27px] leading-[34px] font-medium tracking-[-0.01em]")}>{title}</h1>
              {titleExtra}
            </div>
          ) : (
            <h1 className={cn("text-[34px] leading-[42px] font-light tracking-[-0.03em] break-words text-bright", mono && "font-mono text-[27px] leading-[34px] font-medium tracking-[-0.01em]")}>{title}</h1>
          )}
        </div>
        {/* Beside the h1 from 1280 px; under it (from the left, in one row) when the window is narrower. */}
        {side && <div className="flex flex-none flex-wrap items-center gap-2.5 pt-1">{side}</div>}
      </div>
      {sub && <div className="mb-8 max-w-[46rem] text-[14.5px] leading-[22px] text-muted-foreground">{sub}</div>}
    </header>
  );
}
