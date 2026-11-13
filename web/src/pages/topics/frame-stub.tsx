// A MINIMAL STAND-IN for R1a's topic frame (`pages/topics/frame.tsx`: header, stat strip, actions, tabs as routes), which
// was not on its branch when slice R1b needed it. It has only what R1b's work hangs on: the name, the action buttons that
// open the write overlays by the `dialog` search param, the tabs as routes, and the mounting point of the overlays. At the
// merge R1a's frame replaces this file: its `TopicOverlays` renders `<TopicWriteOverlays .../>` and its Config tab route
// renders `<TopicConfigTab topic=... />`.
import { Link, Outlet, useMatchRoute, useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { ListPlusIcon, SendIcon, Trash2Icon, EraserIcon } from "lucide-react";
import { useRef } from "react";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import type { TopicWriteSearch } from "@/lib/search";
import { cn } from "@/lib/utils";
import { TopicWriteOverlays } from "./writes/overlays";

const TABS = [
  { to: "/topics/$topic", label: "Messages", exact: true },
  { to: "/topics/$topic/log", label: "Log" },
  { to: "/topics/$topic/config", label: "Config" },
  { to: "/topics/$topic/consumers", label: "Consumers" },
  { to: "/topics/$topic/producers", label: "Producers" },
] as const;

export function TopicFrameStub() {
  const { topic } = useParams({ from: "/topics/$topic" });
  const search = useSearch({ from: "/topics/$topic" });
  const navigate = useNavigate();
  const match = useMatchRoute();
  // An overlay opened from this page pushed an entry, so closing goes Back; a deep link has nothing of ours to go back to.
  const pushed = useRef(false);
  const open = (dialog: TopicWriteSearch["dialog"], partition?: number) => {
    pushed.current = true;
    void navigate({ to: ".", search: (s: TopicWriteSearch) => ({ ...s, dialog, partition }), resetScroll: false });
  };
  const close = () => {
    if (pushed.current) {
      pushed.current = false;
      window.history.back();
    } else void navigate({ to: ".", search: (s: TopicWriteSearch) => ({ ...s, dialog: undefined, partition: undefined }), replace: true, resetScroll: false });
  };
  return (
    <>
      <PageHeader
        crumbs={[{ label: "Topics", to: "/topics" }]}
        title={topic}
        mono
        actions={
          <>
            <Button variant="outline" onClick={() => open("add-partitions")} data-testid="open-add-partitions">
              <ListPlusIcon /> Add partitions
            </Button>
            <Button variant="outline" onClick={() => open("delete-records", 0)} data-testid="open-delete-records">
              <EraserIcon /> Delete records
            </Button>
            <Button onClick={() => open("produce")} data-testid="open-produce">
              <SendIcon /> Produce
            </Button>
            <Button variant="outline" className="text-destructive hover:text-destructive" onClick={() => open("delete")} data-testid="open-delete">
              <Trash2Icon /> Delete topic
            </Button>
          </>
        }
      />
      <nav aria-label="Topic" className="mb-6 flex gap-1 border-b" data-testid="topic-tabs">
        {TABS.map((t) => {
          const on = !!match({ to: t.to, params: { topic }, fuzzy: !("exact" in t) });
          return (
            <Link key={t.to} to={t.to} params={{ topic }} className={cn("-mb-px border-b-2 px-3 py-2.5 text-[14px] font-medium", on ? "border-primary text-bright" : "border-transparent text-muted-foreground hover:text-foreground")} aria-current={on ? "page" : undefined}>
              {t.label}
            </Link>
          );
        })}
      </nav>
      <Outlet />
      <TopicWriteOverlays topic={topic} dialog={search.dialog} partition={search.partition} onClose={close} />
    </>
  );
}
