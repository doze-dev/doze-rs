import { SearchIcon } from "lucide-react";
import { cn } from "../../lib/utils";
import { SidebarTrigger } from "../ui/sidebar";

/** The class of a chip in the top bar (an address, a profile, a state): one height, one shape, in both consoles. */
export const topBarChip = "inline-flex h-[30px] items-center gap-2 rounded-full border bg-card px-3 text-[12.5px] whitespace-nowrap text-foreground";

/** The frame's top bar: the sidebar's trigger, the palette's field-shaped button with its key hint, and on the right whatever
 * the app puts there (its chips, the theme switch). The app owns the palette's state; this is its trigger. */
export function TopBar({ onPalette, paletteLabel = "Jump to…", children, className, sidebarTrigger = true }: { onPalette: () => void; /** What the palette's button says (`Jump to a topic or group`). */ paletteLabel?: string; /** The right side. */ children?: React.ReactNode; className?: string; /** False for a frame without the shared Sidebar. */ sidebarTrigger?: boolean }) {
  const mac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  return (
    <header className={cn("sticky top-0 z-20 flex h-[52px] shrink-0 items-center gap-3 border-b bg-background px-4 md:px-6", className)} data-testid="topbar" data-slot="top-bar">
      {sidebarTrigger && <SidebarTrigger className="-ml-1 text-muted-foreground" />}
      <button type="button" onClick={onPalette} className="flex h-[34px] w-full max-w-[22.5rem] min-w-0 items-center gap-2 rounded-[var(--radius-field)] border bg-card pr-2 pl-3 text-[13px] text-muted-foreground transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="palette-trigger" aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}>
        <SearchIcon className="size-4 shrink-0" aria-hidden />
        <span className="truncate">{paletteLabel}</span>
        <kbd className="ml-auto rounded-[5px] border bg-card px-1.5 font-mono text-[11px] leading-[18px]">{mac ? "⌘K" : "Ctrl K"}</kbd>
      </button>
      {children && <div className="ml-auto flex shrink-0 items-center gap-2">{children}</div>}
    </header>
  );
}
