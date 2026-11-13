// The overlays of the Topics pages, routed by `?dialog=` (create on the list; produce, delete, add-partitions,
// delete-records and the config dialogs on a topic). Slice R1b fills these mounting points; R1a renders them, so a dialog
// opens over whichever page it was asked from and Back closes it.
import type { TopicFrame } from "@/api/types/TopicFrame";

/** On every page of one topic (the frame renders it). */
export function TopicOverlays({ frame }: { frame: TopicFrame }) {
  void frame;
  return null;
}

/** On the Topics list (`?dialog=create&name=`). */
export function TopicsOverlays() {
  return null;
}
