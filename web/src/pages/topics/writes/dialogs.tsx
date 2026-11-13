// The four write dialogs of a topic, one open at a time. Each body is keyed by the time it was opened, so a dialog opened
// again starts fresh; the body of the one closing stays until its exit animation ends.
import { useState } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DeleteTopicBody } from "./delete-topic";
import type { TopicDialog } from "./overlays";
import { AddPartitionsBody, DeleteRecordsBody } from "./partitions";
import { ProduceBody } from "./produce";

const TITLES: Record<TopicDialog, { title: (t: string, p?: number) => React.ReactNode; description: string }> = {
  produce: {
    title: (t) => (
      <>
        Produce to <code className="font-mono">{t}</code>
      </>
    ),
    description: "One record. The call below is the one that will be sent; the broker chooses the offset.",
  },
  "add-partitions": {
    title: (t) => (
      <>
        Add partitions to <code className="font-mono">{t}</code>
      </>
    ),
    description: "The new total. Partitions can be added, never removed.",
  },
  "delete-records": {
    title: (t) => (
      <>
        Delete records from <code className="font-mono">{t}</code>
      </>
    ),
    description: "Moves a partition's log start forward. Earlier records are no longer served.",
  },
  delete: {
    title: (t) => (
      <>
        Delete <code className="font-mono">{t}</code>?
      </>
    ),
    description: "The topic, its records and its partitions go from disk.",
  },
};

export default function Dialogs({ topic, dialog, partition, onClose }: { topic: string; dialog?: TopicDialog; partition?: number; onClose: () => void }) {
  // The dialog on screen, kept while the one closing animates out; and when it was opened (the body's key).
  const [shown, setShown] = useState<{ d: TopicDialog; at: number; topic: string } | null>(null);
  const [was, setWas] = useState<TopicDialog | undefined>(undefined);
  if (was !== dialog) {
    setWas(dialog);
    if (dialog) setShown({ d: dialog, at: (shown?.at ?? 0) + 1, topic });
  }
  if (!shown) return null;
  const open = dialog === shown.d;
  const t = TITLES[shown.d];
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(92vh,60rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl" data-testid={`dialog-${shown.d}`} aria-describedby="topic-dialog-d">
        <DialogHeader className="border-b px-6 pt-5 pb-4">
          <DialogTitle className="text-[19px] font-semibold text-bright">{t.title(topic, partition)}</DialogTitle>
          <DialogDescription id="topic-dialog-d">{t.description}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 py-5" data-testid="dialog-body">
          {shown.d === "produce" && <ProduceBody key={shown.at} topic={topic} open={open} />}
          {shown.d === "add-partitions" && <AddPartitionsBody key={shown.at} topic={topic} open={open} />}
          {shown.d === "delete-records" && <DeleteRecordsBody key={shown.at} topic={topic} partition={partition} open={open} />}
          {shown.d === "delete" && <DeleteTopicBody key={shown.at} topic={topic} open={open} />}
        </div>
      </DialogContent>
    </Dialog>
  );
}
