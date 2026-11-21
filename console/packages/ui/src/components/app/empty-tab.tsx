import type { LucideIcon } from "lucide-react";
import { cn } from "../../lib/utils";

/** What a tab says when it has nothing, instead of `None.`: an icon, a heading (what is not there), the consequence (what that
 * means for the thing on the page), and the next action. Centred in a dashed box that holds its height, so a tab with nothing is
 * a place with a way on, not a gap. Quieter than a page's EmptyState: it sits under a resource's tabs. */
export function EmptyTab({ icon: Icon, title, children, action, className, testId = "empty-tab" }: { icon?: LucideIcon; title: string; children?: React.ReactNode; action?: React.ReactNode; className?: string; testId?: string }) {
  return (
    <div className={cn("flex min-h-[16rem] w-full flex-col items-center justify-center gap-2 rounded-[14px] border border-dashed bg-card/60 px-6 py-10 text-center", className)} data-slot="empty-tab" data-testid={testId}>
      {Icon && (
        <span className="mb-2 flex size-12 items-center justify-center rounded-xl bg-muted text-muted-foreground">
          <Icon className="size-5" aria-hidden />
        </span>
      )}
      <h3 className="text-[16px] font-semibold text-bright">{title}</h3>
      {children && <div className="max-w-[28rem] text-[13.5px] leading-[21px] text-muted-foreground">{children}</div>}
      {action && <div className="mt-3 flex flex-wrap items-center justify-center gap-2">{action}</div>}
    </div>
  );
}
