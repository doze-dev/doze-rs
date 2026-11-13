import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { FlaskConicalIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { ApiFailure } from "@/api/client";
import { post, r4keys, useLab } from "@/api/r4";
import type { LabOnce } from "@/api/types/LabOnce";
import type { LabResponse } from "@/api/types/LabResponse";
import type { LabRule } from "@/api/types/LabRule";
import { DataTable, type DataColumn } from "@/components/app/data-table";
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
import { SectionHead, useRoutedOverlay, useStable } from "../r4/bits";
import { NewRehearsalDialog, TAB_WORDS } from "./rule-dialog";
import { END_WORDS, RuleSheet } from "./rule-sheet";

function KindBadge({ kind }: { kind: LabRule["kind"] }) {
  return (
    <Badge variant="secondary" className="font-mono text-[11.5px]">
      {kind}
    </Badge>
  );
}

function Rehearsals({ onStart }: { onStart: (t: LabTab, from: HTMLElement) => void }) {
  return (
    <section aria-labelledby="start" className="mb-12">
      <SectionHead id="start" title="Start a rehearsal" sub="Only what production code can honestly cause. Slow and Fail are rules that end on their own; the others happen once and leave a line of what they did." />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="rehearsal-cards">
        {LAB_TABS.map((t) => {
          const w = TAB_WORDS[t];
          const Icon = w.icon;
          return (
            <button key={t} type="button" onClick={(ev) => onStart(t, ev.currentTarget)} className="group min-w-0 rounded-xl border bg-card p-5 text-left transition-colors hover:border-primary/40 hover:bg-accent/30 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid={`start-${t}`}>
              <div className="flex items-center gap-2">
                <Icon className="size-4 text-warn" aria-hidden />
                <span className="text-[15px] font-semibold text-bright">{w.title}</span>
                <Badge variant="outline" className="ml-auto text-[11px]">
                  {w.once ? "once" : "rule"}
                </Badge>
              </div>
              <p className="mt-2 text-[13px] leading-5 text-muted-foreground">{w.lede}</p>
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
        size: 64,
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
        size: 220,
        cell: (c) => (
          <button type="button" className="max-w-full truncate rounded-sm text-left font-medium text-bright hover:text-primary focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={c.row.original.sentence} onClick={(ev) => open(c.row.original.id, ev.currentTarget)} data-testid="rule-open">
            {c.row.original.name}
          </button>
        ),
      },
      { accessorKey: "kind", header: "Kind", size: 80, cell: (c) => <KindBadge kind={c.row.original.kind} /> },
      { id: "applies", header: "Applies to", accessorFn: (r) => `${r.api} · ${r.target} · ${r.share}`, meta: { mono: true } },
      { accessorKey: "hits", header: "Hits", size: 70, meta: { numeric: true }, cell: (c) => <span data-testid="rule-hits">{number(c.row.original.hits)}</span> },
      { accessorKey: "last_hit_ms", header: "Last hit", size: 92, cell: (c) => <span className="font-mono text-[12.5px]">{c.row.original.last_hit_ms ? clockUtc(c.row.original.last_hit_ms) : "–"}</span> },
      { accessorKey: "ends_ms", header: "Ends", size: 66, cell: (c) => <span className="font-mono text-[12.5px]" title="UTC">{hhmmUtc(c.row.original.ends_ms)}</span> },
      {
        id: "remove",
        header: () => <span className="sr-only">Remove</span>,
        size: 56,
        enableSorting: false,
        cell: (c) => (
          <Button variant="ghost" size="icon-sm" onClick={(e) => (e.stopPropagation(), drop(c.row.original.id, e.currentTarget))} aria-label={`Remove ${c.row.original.name}`} data-testid="rule-remove">
            <Trash2Icon />
          </Button>
        ),
      },
    ],
    [open, flip, drop],
  );
  return <DataTable data={rules} columns={columns} rowId={(r) => String(r.id)} aria-label="Rules in force" maxHeight={480} empty="No rule is in force. Start a rehearsal below." />;
}

const endedColumns = (onOpen: (id: number) => void): DataColumn<LabRule>[] => [
  {
    accessorKey: "name",
    header: "Rule",
    cell: (c) => (
      <button type="button" className="max-w-full truncate rounded-sm text-left font-medium hover:text-primary focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={c.row.original.sentence} onClick={() => onOpen(c.row.original.id)}>
        {c.row.original.name}
      </button>
    ),
  },
  { accessorKey: "kind", header: "Kind", size: 80, cell: (c) => <KindBadge kind={c.row.original.kind} /> },
  { id: "why", header: "How it ended", size: 200, accessorFn: (r) => (r.ended ? END_WORDS[r.ended] : "") },
  { accessorKey: "hits", header: "Hits", size: 80, meta: { numeric: true }, cell: (c) => number(c.row.original.hits) },
  { id: "when", header: "From – to", size: 130, accessorFn: (r) => `${hhmmUtc(r.started_ms)} – ${hhmmUtc(r.ends_ms)}`, meta: { mono: true } },
];

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

export function LabPage() {
  const q = useLab();
  const qc = useQueryClient();
  const search = useSearch({ from: "/lab" });
  const navigate = useNavigate({ from: "/lab" });
  const overlay = useRoutedOverlay((patch, replace) => void navigate({ search: (s: LabSearch) => ({ ...s, ...patch }), replace, resetScroll: false }));
  const [busy, setBusy] = useState(false);
  const openEnded = useStable((id: number) => overlay.open({ rule: id }));
  const endedCols = useMemo(() => endedColumns(openEnded), [openEnded]);
  const r = q.data;
  const h = r?.home;
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["lab"] });
  };
  const fail = (e: unknown) => toast.error(e instanceof ApiFailure ? e.message : String(e));
  const onSwitch = (id: number, on: boolean) => {
    qc.setQueryData<LabResponse>(r4keys.lab, (old) => (old ? { ...old, home: { ...old.home, active: old.home.active.map((a) => (a.id === id ? { ...a, enabled: on } : a)) } } : old));
    post(`/api/v1/lab/rules/${id}/enabled`, { on })
      .then(() => toast.success(on ? "Rule switched on" : "Rule switched off"))
      .catch(fail)
      .finally(refresh);
  };
  const removing = search.remove ? h?.active.find((a) => a.id === search.remove) : undefined;
  const confirmRemove = async () => {
    if (!search.remove) return;
    setBusy(true);
    try {
      await post(`/api/v1/lab/rules/${search.remove}/remove`, {});
      toast.success("Rule removed", { description: removing?.name });
      overlay.close({ remove: undefined });
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
        sub="Rehearse a failure on purpose and watch what your app does. Each rehearsal is a visible rule with an end time, applied where connections are served, and every request it touches shows on the wire with a lab tag."
        refresh={h && !h.off ? <RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} /> : undefined}
        actions={
          h && !h.off ? (
            <Button onClick={(ev) => overlay.open({ new: "slow" }, ev.currentTarget)} data-testid="lab-new">
              <PlusIcon /> New rehearsal
            </Button>
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
          <StatStrip className="mb-10" stats={stats(r!)} />
          {empty ? (
            <EmptyState icon={FlaskConicalIcon} title="Nothing is bent" className="mb-12">
              No rule is in force and nothing ended in the last hour. Start a rehearsal below; every rule ends by itself.
            </EmptyState>
          ) : (
            <section aria-labelledby="active" className="mb-12">
              <SectionHead id="active" title="Active" sub="What is bending your broker right now. Every rule ends by itself; removing it ends it sooner. A rule switched off matches nothing until switched on." />
              <ActiveTable rules={h.active} onOpen={(id, from) => overlay.open({ rule: id }, from)} onSwitch={onSwitch} onRemove={(id, from) => overlay.open({ remove: id }, from)} />
            </section>
          )}
          <Rehearsals onStart={(t, from) => overlay.open({ new: t }, from)} />
          {(h.ended.length > 0 || h.once.length > 0) && (
            <section aria-labelledby="ended" className="mb-12">
              <SectionHead
                id="ended"
                title="Ended"
                sub="Kept for the last hour, so you can look at what a rule did after it is gone."
                actions={
                  <>
                    <Button variant="outline" size="sm" className="bg-card" asChild>
                      <Link to="/live">See the wire on Live</Link>
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
        compactTopics={h?.compacted ?? []}
        restoreFocus={overlay.restore}
        onTab={(t) => overlay.move({ new: t })}
        onClose={() => {
          overlay.close({ new: undefined });
          refresh();
        }}
        onMade={(id) => {
          toast.success("Rehearsal started", { description: `Rule ${id} is in force.` });
          refresh();
          overlay.move({ new: undefined, rule: id });
        }}
      />
      <RuleSheet restoreFocus={overlay.restore} id={search.rule === undefined ? undefined : String(search.rule)} onClose={() => overlay.close({ rule: undefined })} onSwitch={onSwitch} onRemove={(id) => overlay.move({ rule: undefined, remove: id })} />
      <Dialog open={!!search.remove} onOpenChange={(o) => !o && overlay.close({ remove: undefined })}>
        <DialogContent data-testid="lab-remove-dialog" onCloseAutoFocus={overlay.restore}>
          <DialogHeader>
            <DialogTitle>Remove {removing ? removing.name : `rule ${search.remove}`}?</DialogTitle>
            <DialogDescription>It stops at once: the requests it matched are answered as usual from the next one. It stays in Ended for an hour with what it did.</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="outline">Keep it</Button>
            </DialogClose>
            <Button variant="destructive" disabled={busy} onClick={() => void confirmRemove()} data-testid="lab-remove-confirm">
              <Trash2Icon /> Remove rule
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
