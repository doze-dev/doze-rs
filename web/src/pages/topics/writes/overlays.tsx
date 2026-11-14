// The write overlays of a topic (slice R1b): Produce, Add partitions, Delete records, Delete topic. Each is a Dialog over
// the topic's page, routed by the `dialog` search param of the topic's frame (R1a's `TopicOverlays` mounts this). The
// dialogs' code (react-hook-form, the previews) is its own chunk, fetched the first time one opens; once fetched it stays
// mounted, so a dialog animates out when it closes.
import { lazy, Suspense, useState } from "react";

export type TopicDialog = "produce" | "delete" | "add-partitions" | "delete-records";
export const TOPIC_DIALOGS = ["produce", "delete", "add-partitions", "delete-records"] as const;

const Dialogs = lazy(() => import("./dialogs"));

/** The overlay `dialog` names over the topic `topic`; `partition` is Delete records' partition. `onClose` removes the param
 * (Back when the page pushed it). */
export function TopicWriteOverlays({ topic, dialog, partition, onClose }: { topic: string; dialog?: TopicDialog; partition?: number; onClose: () => void }) {
  const [ever, setEver] = useState(false);
  if (dialog && !ever) setEver(true);
  if (!ever && !dialog) return null;
  return (
    <Suspense fallback={null}>
      <Dialogs topic={topic} dialog={dialog} partition={partition} onClose={onClose} />
    </Suspense>
  );
}
