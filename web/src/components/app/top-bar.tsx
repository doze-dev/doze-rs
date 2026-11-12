import { Link } from "@tanstack/react-router";
import { CopyIcon, LockIcon, SearchIcon, TriangleAlertIcon } from "lucide-react";
import { useShell } from "@/api/queries";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { copyText } from "./copy";

const chip = "inline-flex h-[30px] items-center gap-2 rounded-full border bg-card px-3 text-[12.5px] whitespace-nowrap text-foreground";

export function TopBar({ onPalette }: { onPalette: () => void }) {
  const shell = useShell();
  const s = shell.data;
  const mac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
  return (
    <header className="sticky top-0 z-20 flex h-[52px] shrink-0 items-center gap-3 border-b bg-background/95 px-4 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:px-6" data-testid="topbar">
      <SidebarTrigger className="-ml-1 text-muted-foreground" />
      <button type="button" onClick={onPalette} className="flex h-[34px] w-full max-w-[22.5rem] min-w-0 items-center gap-2 rounded-[10px] border bg-card pr-2 pl-3 text-[13px] text-muted-foreground transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="palette-trigger" aria-keyshortcuts={mac ? "Meta+K" : "Control+K"}>
        <SearchIcon className="size-4 shrink-0" aria-hidden />
        <span className="truncate">Jump to a topic or group</span>
        <kbd className="ml-auto rounded-[5px] border bg-card px-1.5 font-mono text-[11px] leading-[18px]">{mac ? "⌘K" : "Ctrl K"}</kbd>
      </button>
      <div className="ml-auto flex min-w-0 items-center gap-2">
        {!s ? (
          <Skeleton className="h-[30px] w-72 rounded-full" />
        ) : (
          <>
            {s.unprotected && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <span className={`${chip} border-warn/40 text-warn`} tabIndex={0}>
                    <TriangleAlertIcon className="size-3.5" /> Unprotected
                  </span>
                </TooltipTrigger>
                <TooltipContent>The console is served beyond this machine with no login (--console-unprotected).</TooltipContent>
              </Tooltip>
            )}
            <Tooltip>
              <TooltipTrigger asChild>
                <button type="button" className={`${chip} font-mono transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none`} onClick={() => void copyText(s.address, "Address copied")} data-testid="address-chip">
                  <span className="size-[7px] rounded-full bg-primary" aria-hidden />
                  {s.address}
                  <CopyIcon className="size-3.5 text-muted-foreground" aria-hidden />
                </button>
              </TooltipTrigger>
              <TooltipContent>Copy the bootstrap address</TooltipContent>
            </Tooltip>
            <span className={`${chip} hidden lg:inline-flex`} data-testid="profile-chip">
              {s.profile}
            </span>
            <Tooltip>
              <TooltipTrigger asChild>
                <Link to="/access" className={`${chip} transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none`} data-testid="security-chip">
                  {s.locked ? <LockIcon className="size-3.5 text-muted-foreground" aria-hidden /> : <span className="size-[7px] rounded-full bg-good" aria-hidden />}
                  {s.security}
                </Link>
              </TooltipTrigger>
              <TooltipContent>Who may connect: see Access</TooltipContent>
            </Tooltip>
          </>
        )}
      </div>
    </header>
  );
}
