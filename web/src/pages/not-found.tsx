import { Link } from "@tanstack/react-router";
import { CompassIcon } from "lucide-react";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";

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
        Jump to a topic or group with ⌘K, or start from Home.
      </EmptyState>
    </>
  );
}
