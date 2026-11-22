// A parameter's History tab (ssm__parameter-history, parameter-labels): the versions (GetParameterHistory) with their labels, and Manage
// labels for the version chosen (LabelParameterVersion, UnlabelParameterVersion).
import { useQueryClient } from "@tanstack/react-query";
import { TagIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { callKindOp, Field, useKindOpRead } from "../../../kit";
import { Note, Say } from "../cfg-parts";
import { PARAMETERS } from "./form";
import { labelProblem, moveWords, type Past, pasts } from "./model";

type History = { Parameters: { Version?: number; LastModifiedDate?: number; Description?: string; Labels?: string[]; Type?: string }[] };
export const useHistory = (name: string, enabled = true) => useKindOpRead<History>(PARAMETERS, "GetParameterHistory", { Name: name }, enabled);

export function TabHistory({ name }: { name: string }) {
  const q = useHistory(name);
  const [version, setVersion] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  if (!q.data) return <Skeleton className="h-48 rounded-[14px]" data-testid="history-loading" />;
  const ps = pasts(q.data.Parameters);
  const chosen = ps.find((p) => p.version === version) ?? null;
  const columns: DataColumn<Past>[] = [
    { id: "pick", size: 44, enableSorting: false, header: () => null, cell: (c) => <input type="radio" name="version" aria-label={`Choose version ${c.row.original.version}`} checked={version === c.row.original.version} onChange={() => setVersion(c.row.original.version)} className="size-4 accent-primary" data-testid={`pick-${c.row.original.version}`} /> },
    { id: "version", header: "Version", accessorKey: "version", size: 90, meta: { numeric: true } },
    { id: "modified", header: "Last modified (UTC)", accessorFn: (p) => p.modified ?? 0, size: 220, cell: (c) => (c.row.original.modified ? dateTimeUtc(c.row.original.modified) : "–") },
    { id: "description", header: "Description", accessorKey: "description", min: 200 },
    { id: "labels", header: "Labels", accessorFn: (p) => p.labels.join(", "), size: 200, cell: (c) => (c.row.original.labels.length > 0 ? <span className="flex flex-wrap gap-1">{c.row.original.labels.map((l) => <span key={l} className="rounded-full border bg-well px-2 py-0.5 font-mono text-[12px]">{l}</span>)}</span> : <span className="text-muted-foreground">None</span>) },
  ];
  return (
    <div className="grid gap-5" data-testid="history-tab">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Say text={`${ps.length} version${ps.length === 1 ? "" : "s"}. Choose one to manage its labels.`} className="text-[13.5px]" testId="history-count" />
        <Button type="button" variant="outline" className="bg-card" disabled={chosen === null} onClick={() => setOpen(true)} data-testid="labels-open"><TagIcon /> Manage labels</Button>
      </div>
      <DataTable aria-label="Versions" data={ps} columns={columns} rowId={(p) => String(p.version)} maxHeight={420} />
      <Note testId="history-note">Every save of the value adds a version and keeps the old ones: {ps.length} exist. The columns are the fields GetParameterHistory returns. On AWS the 100 latest versions are kept: a new version then drops the oldest, unless that oldest version carries a label, and then the save fails until you move the label. A reader that asks for {name}:{ps.flatMap((p) => p.labels)[0] ?? "label"} gets the version that carries that label.</Note>
      {open && chosen && <Labels name={name} past={chosen} all={ps} onClose={() => setOpen(false)} />}
    </div>
  );
}

function Labels({ name, past, all, onClose }: { name: string; past: Past; all: Past[]; onClose: () => void }) {
  const qc = useQueryClient();
  const [labels, setLabels] = useState<string[]>(past.labels);
  const [typed, setTyped] = useState("");
  const [confirm, setConfirm] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<{ code?: string; message: string } | null>(null);
  const added = labels.filter((l) => !past.labels.includes(l));
  const removed = past.labels.filter((l) => !labels.includes(l));
  const problem = typed === "" ? null : labelProblem(typed) ?? (labels.includes(typed) ? `${typed} is already on this version.` : null) ?? (labels.length >= 10 ? "A version can have up to 10 labels." : null);
  const add = () => { if (typed !== "" && !problem) { setLabels([...labels, typed]); setTyped(""); } };
  const needsConfirm = removed.length > 0;
  const save = async () => {
    setPending(true);
    setError(null);
    try {
      if (removed.length > 0) await callKindOp(PARAMETERS, "UnlabelParameterVersion", { Name: name, ParameterVersion: past.version, Labels: removed });
      if (added.length > 0) {
        const r = await callKindOp<{ InvalidLabels?: string[] }>(PARAMETERS, "LabelParameterVersion", { Name: name, ParameterVersion: past.version, Labels: added });
        if (r.InvalidLabels && r.InvalidLabels.length > 0) throw new Error(`${r.InvalidLabels.join(", ")} came back in InvalidLabels and ${r.InvalidLabels.length === 1 ? "was" : "were"} not attached.`);
      }
      await qc.invalidateQueries({ queryKey: ["resources", "kindop", "ssm"] });
      onClose();
    } catch (e) {
      setError(errorWords(e));
    } finally {
      setPending(false);
    }
  };
  const move = moveWords(all, added, past.version);
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent className="sm:max-w-[620px]" data-testid="manage-labels">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Manage labels</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">Labels on version {past.version} of <span className="font-mono">{name}</span>.</DialogDescription>
        </DialogHeader>
        <form className="grid gap-4" noValidate onSubmit={(e) => { e.preventDefault(); if (!needsConfirm || confirm === "Confirm") void save(); }}>
          <div className="grid gap-2"><span className="text-[14px] font-medium text-bright">Labels on version {past.version}</span>
            {labels.length === 0 ? <p className="text-[13px] text-muted-foreground">No labels.</p> : <ul className="flex flex-wrap gap-2">{labels.map((l) => <li key={l} className="inline-flex items-center gap-1 rounded-full border bg-well py-0.5 pr-1 pl-3 font-mono text-[12.5px]">{l}<Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${l}`} onClick={() => setLabels(labels.filter((x) => x !== l))} data-testid={`remove-${l}`}><XIcon /></Button></li>)}</ul>}
          </div>
          <Field label="Label name" error={problem ?? undefined}>{(id) => <div className="flex gap-2"><Input id={id} value={typed} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }} className="bg-card font-mono" data-testid="label-name" /><Button type="button" variant="outline" className="bg-card" onClick={add} data-testid="label-add">Add new label</Button></div>}</Field>
          {move && <Note testId="label-move">{move}</Note>}
          <Note>A version can have up to 10 labels; a label is up to 100 characters of letters (case sensitive), numbers, . - and _, and it can&apos;t begin with a number, aws or ssm; one that breaks a rule is returned in InvalidLabels and isn&apos;t attached.</Note>
          {needsConfirm && <Field label={`Type Confirm to remove ${removed.join(", ")}`}>{(id) => <Input id={id} value={confirm} onChange={(e) => setConfirm(e.target.value)} className="max-w-48 bg-card" data-testid="label-confirm" />}</Field>}
          {error && <Alert tone="bad" title={`The labels were not saved${error.code ? `: ${error.code}` : ""}`} testId="labels-refused">{error.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={pending || (added.length === 0 && removed.length === 0) || (needsConfirm && confirm !== "Confirm")} data-testid="labels-save">{pending ? "Working…" : "Save changes"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
