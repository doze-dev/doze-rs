import { Link } from "@tanstack/react-router";
import { CompassIcon } from "lucide-react";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";

/** An address the console has no page for (the router's not-found component). Never a dead end. */
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
        Pick a space from the rail, or jump to one with the palette.
      </EmptyState>
    </>
  );
}
