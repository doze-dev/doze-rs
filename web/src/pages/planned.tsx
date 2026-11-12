import { Link } from "@tanstack/react-router";
import { CompassIcon, ExternalLinkIcon, HammerIcon } from "lucide-react";
import { useShell } from "@/api/queries";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";

/** A page the React console builds in a later slice (R1 to R4): it says so, and opens the same place in the classic console,
 * which serves every page until R5. Never a dead end. */
export function PlannedPage({ title, slice, classicPath, crumbs = [] }: { title: string; slice: string; classicPath: string; crumbs?: { label: string; to: string }[] }) {
  const shell = useShell();
  const classic = `${(shell.data?.classic ?? "/classic/").replace(/\/$/, "")}${classicPath}`;
  return (
    <>
      <PageHeader title={title} crumbs={crumbs} mono={crumbs.length > 0} />
      <EmptyState
        icon={HammerIcon}
        title="Being rebuilt"
        action={
          <>
            <Button asChild>
              <a href={classic} data-testid="open-classic">
                <ExternalLinkIcon /> Open it in the classic console
              </a>
            </Button>
            <Button variant="outline" asChild>
              <Link to="/">Home</Link>
            </Button>
          </>
        }
      >
        This page moves to the new console in slice {slice}. Until then the classic console has it, with the same data.
      </EmptyState>
    </>
  );
}

export function NotFoundPage() {
  return (
    <>
      <PageHeader title="Not here" />
      <EmptyState
        icon={CompassIcon}
        title="The console has no page at this address"
        action={
          <Button asChild>
            <Link to="/">Home</Link>
          </Button>
        }
      >
        Jump to a topic or group with ⌘K, or start from Home.
      </EmptyState>
    </>
  );
}
