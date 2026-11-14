import { CircleAlertIcon, RefreshCwIcon } from "lucide-react";
import { ApiFailure } from "@/api/client";
import { EmptyState } from "@/components/app/empty-state";
import { Button } from "@/components/ui/button";

/** A page whose data could not be had: what the server said, in its words, and Try again. */
export function PageError({ error, onRetry }: { error: Error; onRetry: () => void }) {
  return (
    <EmptyState
      icon={CircleAlertIcon}
      title="This page could not be loaded"
      action={
        <Button variant="outline" onClick={onRetry}>
          <RefreshCwIcon /> Try again
        </Button>
      }
    >
      {error.message}
      {error instanceof ApiFailure && <span className="mt-1 block font-mono text-[12px]">{error.code}</span>}
    </EmptyState>
  );
}
