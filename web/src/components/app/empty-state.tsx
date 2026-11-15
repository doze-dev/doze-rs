import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";

/** What a place says when it has nothing: an icon, a title, a sentence, and the way on. Reserves its height. */
export function EmptyState({ icon: Icon, title, children, action, className }: { icon: LucideIcon; title: string; children?: React.ReactNode; action?: React.ReactNode; className?: string }) {
  return (
    <div className={cn("flex min-h-56 flex-col items-center justify-center gap-3 rounded-xl border border-dashed bg-card/60 px-6 py-10 text-center", className)} data-slot="empty-state">
      <div className="flex size-11 items-center justify-center rounded-full bg-accent text-link">
        <Icon className="size-5" aria-hidden />
      </div>
      <h3 className="text-[15px] font-semibold text-bright">{title}</h3>
      {children && <div className="max-w-md text-[13.5px] text-muted-foreground">{children}</div>}
      {action && <div className="mt-2 flex flex-wrap items-center justify-center gap-2">{action}</div>}
    </div>
  );
}
