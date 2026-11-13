// The overlays of the Topics pages, routed by `?dialog=` (create on the list; produce, delete, add-partitions and
// delete-records on a topic; the Config tab's edit and reset are `?edit=` and `?reset=` on its own route). R1a renders these
// mounting points, so a dialog opens over whichever page it was asked from; slice R1b fills them.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import type { TopicFrame } from "@/api/types/TopicFrame";
import { whole } from "@/lib/topic-search";
import { useTopicDialog } from "./frame";
import { TOPIC_DIALOGS, type TopicDialog, TopicWriteOverlays } from "./writes/overlays";

const isDialog = (d: string | undefined): d is TopicDialog => !!d && (TOPIC_DIALOGS as readonly string[]).includes(d);

/** On every page of one topic (the frame renders it). */
export function TopicOverlays({ frame }: { frame: TopicFrame }) {
  const { dialog, close } = useTopicDialog();
  const partition = useSearch({ strict: false, select: (s: Record<string, unknown>) => whole(s.partition) });
  // Seen without a dialog first: the dialog was opened from this page, which pushed an entry, so closing goes Back (and
  // Forward reopens it). Opened by a deep link there is nothing of ours to go back to: closing replaces the address.
  const [sawPage, setSawPage] = useState(!dialog);
  if (!dialog && !sawPage) setSawPage(true);
  const shown = isDialog(dialog) ? dialog : undefined;
  return (
    <TopicWriteOverlays
      topic={frame.head.name}
      dialog={shown}
      partition={partition}
      onClose={() => {
        if (sawPage) window.history.back();
        else close();
      }}
    />
  );
}

/** On the Topics list: `?dialog=create&name=` opens Create topic, which is a page of its own (`/topics/new`). */
export function TopicsOverlays() {
  const search = useSearch({ strict: false }) as { dialog?: string; name?: string };
  const navigate = useNavigate();
  const create = search.dialog === "create";
  const name = search.name;
  useEffect(() => {
    if (create) void navigate({ to: "/topics/new", search: { name }, replace: true });
  }, [create, name, navigate]);
  return null;
}
