// The top bar's identity chip: which role is in effect (its display name and colour), or the way to Switch role. The sheet it opens is a chunk
// of its own, read when it is first opened.
import { UserRoundCogIcon } from "lucide-react";
import { lazy, Suspense } from "react";
import { topBarChip } from "@doze/ui/components/app/top-bar";
import { cn } from "@doze/ui/lib/utils";
import { expired, setIdentityOpen, useIdentity } from "./identity";

const Sheet = lazy(() => import("./identity-menu").then((m) => ({ default: m.IdentitySheet })));

const DOT = { red: "bg-bad", green: "bg-good" } as const;

export function IdentityChip() {
  const { session, open } = useIdentity();
  const live = session && !expired(session) ? session : null;
  return (
    <>
      <button type="button" onClick={() => setIdentityOpen(true)} className={cn(topBarChip, "hidden outline-none transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 lg:inline-flex")} aria-label={live ? `Identity: ${live.display}` : "Identity menu"} data-testid="chip-identity">
        {live ? <span aria-hidden className={cn("size-[7px] shrink-0 rounded-full", live.color ? DOT[live.color] : "bg-primary")} /> : <UserRoundCogIcon aria-hidden className="size-3.5 text-muted-foreground" />}
        <span className={cn(live && live.color === "red" && "text-bad", live && live.color === "green" && "text-good")}>{live ? live.display : "Switch role"}</span>
      </button>
      {open && (
        <Suspense fallback={null}>
          <Sheet />
        </Suspense>
      )}
    </>
  );
}
