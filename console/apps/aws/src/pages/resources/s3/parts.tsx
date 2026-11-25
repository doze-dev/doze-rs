// Small pieces the S3 pages share: the layout of a full-page action without a call panel, a read's definition list, the result a
// page was left with, and the loading and refusal states of a read.
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { PageHeader, type Crumb } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import { dismissFlash, type Flash, takeFlash } from "./flash";

/** The result the previous page left (`Bucket created.`), read when this page is first drawn. */
export function useFlash(): [Flash | null, () => void] {
  const [flash, setFlash] = useState<Flash | null>(() => takeFlash());
  return [flash, () => (dismissFlash(), setFlash(null))];
}

export function FlashBanner({ flash, dismiss }: { flash: Flash | null; dismiss: () => void }) {
  if (!flash) return null;
  return <ResultBanner tone={flash.tone ?? "good"} title={flash.title} returned={flash.returned} left={flash.left} onDismiss={dismiss} testId="flash" />;
}

/** A full page of one action that is not a form with a call (a delete, a status): the header, a banner, the sections, and the
 * buttons at the foot. */
export function ActionPage({ title, sub, crumbs, banner, actions, children, footer, mono = false, testId = "action-page" }: { title: string; sub?: React.ReactNode; crumbs?: Crumb[]; banner?: React.ReactNode; actions?: React.ReactNode; children?: React.ReactNode; footer?: React.ReactNode; mono?: boolean; testId?: string }) {
  return (
    <div data-testid={testId}>
      <PageHeader title={title} sub={sub} crumbs={crumbs} actions={actions} mono={mono} />
      {banner && <div className="mb-6">{banner}</div>}
      <div className="grid gap-6">{children}</div>
      {footer && <div className="mt-6 flex flex-wrap items-center justify-end gap-3 border-t pt-5">{footer}</div>}
    </div>
  );
}

/** A label and its value, a row each. */
export function Rows({ rows, className, testId }: { rows: [string, React.ReactNode][]; className?: string; testId?: string }) {
  return (
    <dl className={cn("grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start text-[13.5px] [&>div:last-child>*]:border-b-0", className)} data-testid={testId}>
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="border-b py-3 pr-4 text-muted-foreground">{k}</dt>
          <dd className="min-w-0 border-b py-3 break-words text-bright" data-testid={`row-${k}`}>
            {v ?? <span className="text-muted-foreground">–</span>}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export const Mono = ({ children }: { children: React.ReactNode }) => <span className="font-mono text-[13px]">{children}</span>;

/** What a read that is not there yet or was refused shows in place of its body. */
export function Reading({ q, what, children, height = "h-32" }: { q: { data: unknown; isPending: boolean; failed?: unknown; error?: unknown; refetch: () => unknown; isFetching: boolean }; what: string; children: React.ReactNode; height?: string }) {
  const failure = q.failed ?? (q.data === undefined ? q.error : null);
  if (failure) return <ErrorBlock what={`${what} could not be read`} error={failure} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="read-error" />;
  if (q.isPending) return <Skeleton className={cn("rounded-[var(--radius-panel)]", height)} data-testid="reading" />;
  return <>{children}</>;
}

/** Cancel (a way out), then the button that does it. */
export function Footer({ cancel, onCancel, children }: { cancel?: string; onCancel: () => void; children: React.ReactNode }) {
  return (
    <>
      <Button type="button" variant="outline" className="bg-card" onClick={onCancel} data-testid="form-cancel">
        {cancel ?? "Cancel"}
      </Button>
      {children}
    </>
  );
}

/** What a section says when it has nothing: a heading, the consequence, and the way on. Small: it sits inside a card. */
export function NoneYet({ title, children, action, icon: Icon, testId = "none-yet" }: { title: string; children?: React.ReactNode; action?: React.ReactNode; /** An icon in a tile at the left, as the empty tabs have one. */ icon?: import("lucide-react").LucideIcon; testId?: string }) {
  return (
    <div className="flex items-start gap-4 rounded-xl border border-dashed bg-well/50 px-5 py-4" data-testid={testId}>
      {Icon && (
        <span className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground">
          <Icon className="size-[18px]" aria-hidden />
        </span>
      )}
      <div className="grid justify-items-start gap-1.5">
        <h3 className="text-[15px] font-semibold text-bright">{title}</h3>
        {children && <p className="max-w-[40rem] text-[13.5px] leading-[21px] text-muted-foreground">{children}</p>}
        {action && <div className="mt-1.5">{action}</div>}
      </div>
    </div>
  );
}
