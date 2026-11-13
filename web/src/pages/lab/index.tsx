import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { ActivityIcon, FlaskConicalIcon, OctagonXIcon, PlusIcon, ScrollTextIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { ApiFailure } from "@/api/client";
import { segment } from "@/api/path";
import { post, r4keys, useLab, useLabGuide } from "@/api/r4";
import type { LabGuide } from "@/api/types/LabGuide";
import type { LabOnce } from "@/api/types/LabOnce";
import type { LabResponse } from "@/api/types/LabResponse";
import type { LabRule } from "@/api/types/LabRule";
import type { LabStopped } from "@/api/types/LabStopped";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { Done, DoneFooter } from "@/components/app/write-done";
import { EmptyState } from "@/components/app/empty-state";
import { PageHeader } from "@/components/app/page-header";
import { RefreshControl } from "@/components/app/refresh-control";
import { StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { clockUtc, hhmmUtc, number } from "@/lib/format";
import { LAB_TABS, type LabSearch, type LabTab } from "@/lib/search";
import { PageError } from "../page-error";
import { narrowed, SectionHead, useMinWidth, useRoutedOverlay, useStable } from "../r4/bits";
import { HowItWorks, Recipes, TAB_LOOK, teachOf } from "./guide";
import { NewRehearsalDialog } from "./rule-dialog";
import { END_WORDS, RuleSheet } from "./rule-sheet";

function KindBadge({ kind }: { kind: LabRule["kind"] }) {
  return (
    <Badge variant="secondary" className="font-mono text-[11.5px]">
      {kind}
    </Badge>
  );
}

/** The six rehearsals, to build one from scratch: what each does and what it stands for in production. */
function Rehearsals({ guide, onStart }: { guide: LabGuide | undefined; onStart: (t: LabTab, from: HTMLElement) => void }) {
  return (
    <section aria-labelledby="start" className="mb-12">
      <SectionHead id="start" title="Build your own" sub="Only what production can honestly cause. Slow and Fail are rules that end on their own; the others happen once and leave a line of what they did." />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="rehearsal-cards">
        {LAB_TABS.map((t) => {
          const look = TAB_LOOK[t];
          const teach = teachOf(guide, t);
          const Icon = look.icon;
          return (
            <button key={t} type="button" onClick={(ev) => onStart(t, ev.currentTarget)} className="group flex min-w-0 flex-col rounded-xl border bg-card p-5 text-left transition-colors hover:border-primary/40 hover:bg-accent/30 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid={`start-${t}`}>
              <span className="flex w-full items-center gap-2">
                <Icon className="size-4 text-warn" aria-hidden />
                <span className="text-[15px] font-semibold text-bright">{teach?.title ?? look.label}</span>
                <Badge variant="outline" className="ml-auto text-[11px]">
                  {look.once ? "once" : "rule"}
                </Badge>
              </span>
              {teach ? (
                <>
                  <span className="mt-2 text-[13px] leading-5 text-muted-foreground">{teach.lede}</span>
                  <span className="mt-3 border-t pt-3 text-[12.5px] leading-[19px] text-muted-foreground">
                    <b className="font-semibold text-foreground">In production: </b>
                    {teach.simulates}
                  </span>
                </>
              ) : (
                <Skeleton className="mt-2 h-20 w-full" />
              )}
            </button>
          );
        })}
      </div>
    </section>
  );
}

function ActiveTable({ rules, onOpen, onSwitch, onRemove }: { rules: LabRule[]; onOpen: (id: number, from: HTMLElement) => void; onSwitch: (id: number, on: boolean) => void; onRemove: (id: number, from: HTMLElement) => void }) {
  const [open, flip, drop] = [useStable(onOpen), useStable(onSwitch), useStable(onRemove)];
  const columns = useMemo<DataColumn<LabRule>[]>(
    () => [
      {
        accessorKey: "enabled",
        header: "On",
        size: 60,
        enableSorting: false,
        cell: (c) => (
          <span onClick={(e) => e.stopPropagation()} className="inline-flex">
            <Switch checked={c.row.original.enabled} onCheckedChange={(on) => flip(c.row.original.id, on)} aria-label={`${c.row.original.name}: on`} data-testid="rule-switch" />
          </span>
        ),
      },
      {
        accessorKey: "name",
        header: "Rule",
        size: 190,
        cell: (c) => (
          <button type="button" className="max-w-full truncate rounded-sm text-left font-medium text-bright hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={c.row.original.sentence} onClick={(ev) => open(c.row.original.id, ev.currentTarget)} data-testid="rule-open">
            {c.row.original.name}
          </button>
        ),
      },
      { accessorKey: "kind", header: "Kind", size: 64, cell: (c) => <KindBadge kind={c.row.original.kind} /> },
      { id: "applies", header: "Applies to", accessorFn: (r) => `${r.api} · ${r.target} · ${r.share}`, meta: { mono: true } },
      { accessorKey: "hits", header: "Hits", size: 60, meta: { numeric: true }, cell: (c) => <span data-testid="rule-hits">{number(c.row.original.hits)}</span> },
      { accessorKey: "last_hit_ms", header: "Last hit", size: 88, cell: (c) => <span className="font-mono text-[12.5px]">{c.row.original.last_hit_ms ? clockUtc(c.row.original.last_hit_ms) : "–"}</span> },
      { accessorKey: "ends_ms", header: "Ends", size: 60, cell: (c) => <span className="font-mono text-[12.5px]" title="UTC">{hhmmUtc(c.row.original.ends_ms)}</span> },
      {
        id: "watch",
        header: () => <span className="sr-only">Watch</span>,
        size: 44,
        enableSorting: false,
        cell: (c) => (
          <Button variant="ghost" size="icon-sm" asChild>
            <Link to="/live" search={{ lab: c.row.original.id }} onClick={(e) => e.stopPropagation()} aria-label={`See ${c.row.original.name} on Live`} title="See its requests on Live" data-testid="rule-watch">
              <ActivityIcon />
            </Link>
          </Button>
        ),
      },
      {
        id: "remove",
        header: () => <span className="sr-only">Remove</span>,
        size: 44,
        enableSorting: false,
        cell: (c) => (
          <Button variant="ghost" size="icon-sm" onClick={(e) => (e.stopPropagation(), drop(c.row.original.id, e.currentTarget))} aria-label={`Remove ${c.row.original.name}`} title="Remove it now" data-testid="rule-remove">
            <Trash2Icon />
          </Button>
        ),
      },
    ],
    [open, flip, drop],
  );
  const wide = useMinWidth(1280);
  return <DataTable data={rules} columns={narrowed(columns, wide, ["kind", "applies"], "name")} rowId={(r) => String(r.id)} aria-label="Rules in force" maxHeight={480} empty="No rule is in force. Start a rehearsal below." />;
}

const endedColumns = (onOpen: (id: number) => void): DataColumn<LabRule>[] => [
  {
    accessorKey: "name",
    header: "Rule",
    cell: (c) => (
      <button type="button" className="max-w-full truncate rounded-sm text-left font-medium hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={c.row.original.sentence} onClick={() => onOpen(c.row.original.id)}>
        {c.row.original.name}
      </button>
    ),
  },
  { accessorKey: "kind", header: "Kind", size: 80, cell: (c) => <KindBadge kind={c.row.original.kind} /> },
  { id: "why", header: "How it ended", size: 200, accessorFn: (r) => (r.ended ? END_WORDS[r.ended] : "") },
  { accessorKey: "hits", header: "Hits", size: 80, meta: { numeric: true }, cell: (c) => number(c.row.original.hits) },
  { id: "when", header: "From – to", size: 130, accessorFn: (r) => `${hhmmUtc(r.started_ms)} – ${hhmmUtc(r.ends_ms)}`, meta: { mono: true } },
];

/** Where a line of the history leads: a group's Story, a topic's Log. */
function OnceLink({ o }: { o: LabOnce }) {
  if (!o.subject || !o.ok) return null;
  if (o.kind === "expire" || o.kind === "rebalance") {
    return (
      <Link to="/groups/$group/story" params={{ group: o.subject }} className="inline-flex items-center gap-1 text-[13px] font-medium whitespace-nowrap text-link hover:underline" data-testid="once-story">
        <ScrollTextIcon className="size-3.5" aria-hidden /> Story
      </Link>
    );
  }
  if (o.kind === "clean") {
    return (
      <Link to="/topics/$topic/log" params={{ topic: o.subject }} className="inline-flex items-center gap-1 text-[13px] font-medium whitespace-nowrap text-link hover:underline">
        <ScrollTextIcon className="size-3.5" aria-hidden /> Log
      </Link>
    );
  }
  return null;
}

function OnceList({ once }: { once: LabOnce[] }) {
  if (once.length === 0) return null;
  return (
    <Card className="mt-4 min-w-0">
      <CardContent className="py-2">
        <ul data-testid="once-list">
          {once.map((o) => (
            <li key={o.id} className="flex items-baseline gap-3 border-b py-2.5 text-[13.5px] last:border-b-0">
              <span className="font-mono text-[12.5px] text-muted-foreground">{clockUtc(o.at_ms)}</span>
              <Badge variant="outline" className="font-mono text-[11px]">
                {o.kind}
              </Badge>
              <span className="min-w-0 flex-1 break-words">{o.sentence}</span>
              <OnceLink o={o} />
              <StatusDot tone={o.ok ? "ok" : "bad"} className="font-medium">
                {o.ok ? "done" : "refused"}
              </StatusDot>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function stats(r: LabResponse) {
  const h = r.home;
  return [
    { label: "Rules in force", value: `${h.active.length} of ${h.most_rules}`, unit: `${h.active.filter((a) => a.enabled).length} switched on`, ch: 7 },
    { label: "Hits", value: number(h.active.reduce((n, a) => n + a.hits, 0)), unit: "by the rules in force", ch: 6 },
    { label: "Ended", value: number(h.ended.length), unit: "in the last hour", ch: 4 },
    { label: "Done once", value: number(h.once.length), unit: "drops, expiries, rebalances, cleans", ch: 4 },
  ];
}

/** Stop all (`/lab?stop=all`): every rule in force ends now; the result in place, then Done. */
function StopAllDialog({ open, onClose, onStopped }: { open: boolean; onClose: () => void; onStopped: () => void }) {
  const [busy, setBusy] = useState(false);
  const [stopped, setStopped] = useState<number | null>(null);
  const stop = async () => {
    setBusy(true);
    try {
      const out = await post<LabStopped>("/api/v1/lab/stop-all", {});
      setStopped(out.stopped);
    } catch (e) {
      toast.error(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
      onStopped();
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) {
          setStopped(null);
          onClose();
        }
      }}
    >
      <DialogContent data-testid="lab-stop-dialog">
        <DialogHeader>
          <DialogTitle>Stop every rehearsal?</DialogTitle>
          <DialogDescription>Every rule in force ends now: from the next request, the broker answers as usual. They stay in Ended for an hour with what they did.</DialogDescription>
        </DialogHeader>
        {stopped !== null ? (
          <>
            <Done testId="lab-stopped">
              <b className="font-semibold text-bright">Stopped.</b> {stopped === 1 ? "1 rule ended." : `${number(stopped)} rules ended.`} Nothing in the Lab bends the broker now.
            </Done>
            <DoneFooter
              onDone={() => {
                setStopped(null);
                onClose();
              }}
            />
          </>
        ) : (
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">Keep them</Button>
            </DialogClose>
            <Button variant="destructive" disabled={busy} onClick={() => void stop()} data-testid="lab-stop-confirm">
              <OctagonXIcon /> Stop all
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  );
}

export function LabPage() {
  const q = useLab();
  const g = useLabGuide();
  const guide = g.data;
  const qc = useQueryClient();
  const search = useSearch({ from: "/lab" });
  const navigate = useNavigate({ from: "/lab" });
  const overlay = useRoutedOverlay((patch, replace) => void navigate({ search: (s: LabSearch) => ({ ...s, ...patch }), replace, resetScroll: false }));
  const [busy, setBusy] = useState(false);
  // A rule removed from the Remove dialog: the result in place, then Done (one pattern for every write).
  const [removedRule, setRemovedRule] = useState<{ id: number; name: string } | null>(null);
  const setRemoved = (name: string) => search.remove !== undefined && setRemovedRule({ id: search.remove, name });
  const removed = removedRule && removedRule.id === search.remove ? removedRule.name : null;
  const openEnded = useStable((id: number) => overlay.open({ rule: id }));
  const endedCols = useMemo(() => endedColumns(openEnded), [openEnded]);
  const r = q.data;
  const h = r?.home;
  const refresh = () => {
    // The page and the guide (its recipes name what the broker has now).
    void qc.invalidateQueries({ queryKey: ["lab"] });
  };
  const fail = (e: unknown) => toast.error(e instanceof ApiFailure ? e.message : String(e));
  const onSwitch = (id: number, on: boolean) => {
    qc.setQueryData<LabResponse>(r4keys.lab, (old) => (old ? { ...old, home: { ...old.home, active: old.home.active.map((a) => (a.id === id ? { ...a, enabled: on } : a)) } } : old));
    post(`/api/v1/lab/rules/${segment(id)}/enabled`, { on })
      .then(() => toast.success(on ? "Rule switched on" : "Rule switched off"))
      .catch(fail)
      .finally(refresh);
  };
  const removing = search.remove ? h?.active.find((a) => a.id === search.remove) : undefined;
  const confirmRemove = async () => {
    if (!search.remove) return;
    setBusy(true);
    try {
      await post(`/api/v1/lab/rules/${segment(search.remove)}/remove`, {});
      setRemoved(removing?.name ?? `Rule ${search.remove}`);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      refresh();
    }
  };
  const clearHistory = () => {
    post("/api/v1/lab/history/clear", {})
      .then(() => toast.success("History cleared"))
      .catch(fail)
      .finally(refresh);
  };
  const empty = h ? h.active.length === 0 && h.ended.length === 0 && h.once.length === 0 : false;
  return (
    <>
      <PageHeader
        title="Lab"
        sub="Rehearse the failures production will bring, on your laptop, and watch what your app does. Every request a rehearsal touches shows on Live with a lab tag."
        refresh={h && !h.off ? <RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} /> : undefined}
        actions={
          h && !h.off ? (
            <>
              <Button variant="outline" className="bg-card" disabled={h.active.length === 0} onClick={(ev) => overlay.open({ stop: "all" }, ev.currentTarget)} title={h.active.length === 0 ? "No rule is in force" : "End every rule in force now"} data-testid="lab-stop-all">
                <OctagonXIcon /> Stop all
              </Button>
              <Button onClick={(ev) => overlay.open({ new: "slow" }, ev.currentTarget)} data-testid="lab-new">
                <PlusIcon /> New rehearsal
              </Button>
            </>
          ) : undefined
        }
      />
      {q.error && !r ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !h ? (
        <Skeleton className="h-96 w-full rounded-xl" />
      ) : h.off ? (
        <EmptyState icon={FlaskConicalIcon} title="The Lab is off on this broker">
          It was started with <code className="font-mono">--lab=false</code>, so no fault can be injected. Start it without the flag (the Lab is on by default on loopback) to rehearse failures.
        </EmptyState>
      ) : (
        <div data-state={empty ? "empty" : h.active.length ? "active" : "expired"} data-testid="lab-home">
          <HowItWorks intro={guide?.intro} />
          <StatStrip className="mb-10" stats={stats(r!)} />
          {empty ? (
            <EmptyState icon={FlaskConicalIcon} title="Nothing is bent" className="mb-12 min-h-0 py-8">
              No rule is in force and nothing ended in the last hour. Start from a recipe below, or build your own; every rule ends by itself.
            </EmptyState>
          ) : (
            <section aria-labelledby="active" className="mb-12">
              <SectionHead id="active" title="Active" sub="What is bending your broker right now. Every rule ends by itself; removing it ends it sooner. A rule switched off matches nothing until switched on." />
              <ActiveTable rules={h.active} onOpen={(id, from) => overlay.open({ rule: id }, from)} onSwitch={onSwitch} onRemove={(id, from) => overlay.open({ remove: id }, from)} />
            </section>
          )}
          <Recipes recipes={guide?.recipes} onUse={(rc, from) => overlay.open({ new: rc.tab as LabTab, recipe: rc.id }, from)} />
          <Rehearsals guide={guide} onStart={(t, from) => overlay.open({ new: t }, from)} />
          {(h.ended.length > 0 || h.once.length > 0) && (
            <section aria-labelledby="ended" className="mb-12">
              <SectionHead
                id="ended"
                title="Ended"
                sub="Kept for the last hour, so you can look at what a rule did after it is gone."
                actions={
                  <>
                    <Button variant="outline" size="sm" className="bg-card" asChild>
                      <Link to="/live" search={{ lab: "all" }}>
                        <ActivityIcon /> Every lab request on Live
                      </Link>
                    </Button>
                    <Button variant="outline" size="sm" className="bg-card" onClick={clearHistory} data-testid="lab-clear">
                      <Trash2Icon /> Clear history
                    </Button>
                  </>
                }
              />
              {h.ended.length > 0 && <DataTable data={h.ended} columns={endedCols} rowId={(x) => `e${x.id}`} aria-label="Rules that ended" maxHeight={360} />}
              <OnceList once={h.once} />
            </section>
          )}
        </div>
      )}
      <NewRehearsalDialog
        tab={h && !h.off ? search.new : undefined}
        recipe={search.recipe}
        guide={guide}
        compactTopics={h?.compacted ?? []}
        onTab={(t) => overlay.move({ new: t, recipe: undefined })}
        onClose={() => {
          overlay.close({ new: undefined, recipe: undefined });
          refresh();
        }}
        onMade={(id) => {
          // The result is the rule's own sheet, opened in place of the dialog.
          refresh();
          overlay.move({ new: undefined, recipe: undefined, rule: id });
        }}
      />
      <RuleSheet id={search.rule === undefined ? undefined : String(search.rule)} onClose={() => overlay.close({ rule: undefined })} onSwitch={onSwitch} onRemove={(id) => overlay.move({ rule: undefined, remove: id })} />
      <StopAllDialog open={search.stop === "all" && !!h && !h.off} onClose={() => overlay.close({ stop: undefined })} onStopped={refresh} />
      <Dialog open={!!search.remove} onOpenChange={(o) => !o && overlay.close({ remove: undefined })}>
        <DialogContent data-testid="lab-remove-dialog">
          <DialogHeader>
            <DialogTitle>Remove {removed ?? (removing ? removing.name : `rule ${search.remove}`)}?</DialogTitle>
            <DialogDescription>It stops at once: the requests it matched are answered as usual from the next one. It stays in Ended for an hour with what it did.</DialogDescription>
          </DialogHeader>
          {removed ? (
            <>
              <Done testId="lab-removed">
                <b className="font-semibold text-bright">Removed {removed}.</b> It is in Ended now, with what it did.
              </Done>
              <DoneFooter onDone={() => overlay.close({ remove: undefined })} />
            </>
          ) : (
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">Keep it</Button>
            </DialogClose>
            <Button variant="destructive" disabled={busy} onClick={() => void confirmRemove()} data-testid="lab-remove-confirm">
              <Trash2Icon /> Remove rule
            </Button>
          </DialogFooter>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
