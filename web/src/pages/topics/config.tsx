// The Config tab of a topic: slice R1b builds it (with its edit and reset dialogs). Until then it opens the classic page.
import { useParams } from "@tanstack/react-router";
import { ExternalLinkIcon, HammerIcon } from "lucide-react";
import { useShell } from "@/api/queries";
import { EmptyState } from "@/components/app/empty-state";
import { Button } from "@/components/ui/button";

export function TopicConfigPage() {
  const { topic } = useParams({ from: "/topics/$topic/config" });
  const shell = useShell();
  const classic = `${(shell.data?.classic ?? "/classic/").replace(/\/$/, "")}/topics/${encodeURIComponent(topic)}/config`;
  return (
    <EmptyState
      icon={HammerIcon}
      title="Being rebuilt"
      action={
        <Button asChild>
          <a href={classic}>
            <ExternalLinkIcon /> Open it in the classic console
          </a>
        </Button>
      }
    >
      The topic's configs and their dialogs come in the next slice.
    </EmptyState>
  );
}
