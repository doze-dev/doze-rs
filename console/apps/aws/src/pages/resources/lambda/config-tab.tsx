// A function's Configuration tab: its general settings and environment variables as GetFunctionConfiguration answers them,
// each edited in a dialog over the page and saved with UpdateFunctionConfiguration (only what changed is sent; the variables
// are one map, sent whole).
import { PencilIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { on, useOp, useOpRead } from "../deep/api";
import { at, Card, Loaded, Made, Refused } from "../deep/parts";

export type FunctionConfiguration = { FunctionName: string; Description?: string; MemorySize?: number; Timeout?: number; Runtime?: string; Handler?: string; Role?: string; EphemeralStorage?: { Size?: number }; Environment?: { Variables?: Record<string, string> }; LastModified?: string; State?: string; LastUpdateStatus?: string };

/** A key AWS takes: a letter, then letters, digits and underscores, two characters at least. */
export const ENV_KEY = /^[a-zA-Z][a-zA-Z0-9_]+$/;

/** What the general form changes, as UpdateFunctionConfiguration's members; `null` when a value is out of range. */
export function generalChange(c: FunctionConfiguration, form: { description: string; memory: string; timeout: string }): Record<string, unknown> | null {
  const memory = Number(form.memory);
  const timeout = Number(form.timeout);
  if (!Number.isInteger(memory) || memory < 128 || memory > 10240) return null;
  if (!Number.isInteger(timeout) || timeout < 1 || timeout > 900) return null;
  const out: Record<string, unknown> = {};
  if (form.description !== (c.Description ?? "")) out.Description = form.description;
  if (memory !== c.MemorySize) out.MemorySize = memory;
  if (timeout !== c.Timeout) out.Timeout = timeout;
  return out;
}

function Row({ k, v }: { k: React.ReactNode; v: React.ReactNode }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{k}</dt>
      <dd className="border-b py-3 text-[13.5px] break-words text-bright">{v}</dd>
    </div>
  );
}

function GeneralDialog({ c, open, onOpenChange }: { c: FunctionConfiguration; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [form, setForm] = useState({ description: c.Description ?? "", memory: String(c.MemorySize ?? 128), timeout: String(c.Timeout ?? 3) });
  const save = useOp<FunctionConfiguration>(on.fn(c.FunctionName), "UpdateFunctionConfiguration");
  const change = generalChange(c, form);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!change || Object.keys(change).length === 0) return;
    save.mutate({ FunctionName: c.FunctionName, ...change }, { onSuccess: () => { onOpenChange(false); toast.success("Configuration saved"); } });
  };
  const field = (id: keyof typeof form, label: string, help: string, unit?: string) => (
    <div className="grid gap-1.5">
      <Label htmlFor={`cfg-${id}`}>{label}</Label>
      <div className="flex items-center gap-2">
        <Input id={`cfg-${id}`} inputMode="numeric" value={form[id]} onChange={(e) => setForm({ ...form, [id]: e.target.value })} className="bg-card font-mono" data-testid={`cfg-${id}`} />
        {unit && <span className="text-[13px] text-muted-foreground">{unit}</span>}
      </div>
      <p className="text-[12.5px] text-muted-foreground">{help}</p>
    </div>
  );
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[600px]" data-testid="config-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Edit general configuration</DialogTitle>
            <DialogDescription>A saved change applies to $LATEST; published versions keep the settings they had.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="cfg-description">
              Description <span className="ml-1 font-normal text-muted-foreground">optional</span>
            </Label>
            <Textarea id="cfg-description" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} className="min-h-16 bg-card" maxLength={256} />
            <p className="text-[12.5px] text-muted-foreground">Up to 256 characters.</p>
          </div>
          {field("memory", "Memory", "From 128 MB to 10,240 MB. Stored and reported; doze-aws does not cap the process's memory.", "MB")}
          {field("timeout", "Timeout", "From 1 to 900 seconds. A run that takes longer is stopped and answered as a timeout.", "seconds")}
          {!change && <p className="text-[12.5px] text-bad">Memory is a whole number from 128 to 10240, and the timeout one from 1 to 900.</p>}
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            Saved with <span className="font-mono">UpdateFunctionConfiguration</span>; settings you did not change are left out.
          </p>
          <Refused error={save.error} operation="UpdateFunctionConfiguration" testId="write-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={!change || Object.keys(change).length === 0 || save.isPending} data-testid="config-save">{save.isPending ? "Saving…" : "Save"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function EnvDialog({ c, open, onOpenChange }: { c: FunctionConfiguration; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [rows, setRows] = useState<[string, string][]>(() => Object.entries(c.Environment?.Variables ?? {}));
  const save = useOp<FunctionConfiguration>(on.fn(c.FunctionName), "UpdateFunctionConfiguration");
  const keys = rows.map(([k]) => k);
  const bad = rows.some(([k]) => !ENV_KEY.test(k)) || new Set(keys).size !== keys.length;
  const size = rows.reduce((s, [k, v]) => s + k.length + v.length, 0);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (bad || size > 4096) return;
    save.mutate({ FunctionName: c.FunctionName, Environment: { Variables: Object.fromEntries(rows) } }, { onSuccess: () => { onOpenChange(false); toast.success("Environment variables saved"); } });
  };
  const set = (i: number, at: 0 | 1, v: string) => setRows(rows.map((r, j) => (j === i ? ((at === 0 ? [v, r[1]] : [r[0], v]) as [string, string]) : r)));
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[720px]" data-testid="env-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Edit environment variables</DialogTitle>
            <DialogDescription>The function's process gets these at its next start; a change applies to $LATEST.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-2" data-testid="env-rows">
            {rows.length === 0 && <p className="text-[13.5px] text-muted-foreground">No variables. Add one, or save none.</p>}
            {rows.map(([k, v], i) => (
              <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto] items-center gap-2">
                <Input aria-label={`Key ${i + 1}`} value={k} onChange={(e) => set(i, 0, e.target.value)} aria-invalid={!ENV_KEY.test(k)} className="bg-card font-mono" placeholder="e.g. LOG_LEVEL" data-testid="env-key" />
                <Input aria-label={`Value ${i + 1}`} value={v} onChange={(e) => set(i, 1, e.target.value)} className="bg-card font-mono" data-testid="env-value" />
                <Button type="button" variant="ghost" size="sm" onClick={() => setRows(rows.filter((_, j) => j !== i))} aria-label={`Remove ${k || `row ${i + 1}`}`}>
                  <Trash2Icon />
                </Button>
              </div>
            ))}
            <div>
              <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setRows([...rows, ["", ""]])} data-testid="env-add">
                <PlusIcon /> Add variable
              </Button>
            </div>
          </div>
          <p className={bad || size > 4096 ? "text-[12.5px] text-bad" : "text-[12.5px] text-muted-foreground"}>
            A key starts with a letter, has two characters at least, and holds letters, digits and underscores; keys are unique. Keys and values together are at most 4 KB ({size} bytes now).
          </p>
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            Saved with <span className="font-mono">UpdateFunctionConfiguration</span>: the whole map is replaced, so every variable kept is sent again.
          </p>
          <Refused error={save.error} operation="UpdateFunctionConfiguration" testId="write-error" />
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={bad || size > 4096 || save.isPending} data-testid="env-save">{save.isPending ? "Saving…" : "Save"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function FunctionConfig({ name }: { name: string }) {
  const q = useOpRead<FunctionConfiguration>(on.fn(name), "GetFunctionConfiguration", { FunctionName: name });
  const [editing, setEditing] = useState<"general" | "env" | null>(null);
  // A dialog opened again starts from what is saved: a new key each time it opens.
  const [opened, setOpened] = useState(0);
  const edit = (which: "general" | "env") => {
    setOpened((n) => n + 1);
    setEditing(which);
  };
  return (
    <Loaded q={q} what="The function's configuration">
      {(c) => {
        const vars = Object.entries(c.Environment?.Variables ?? {});
        return (
          <div className="grid gap-6" data-testid="lambda-config">
            <Card title="General configuration" lede={`How ${c.FunctionName} runs. Last modified ${at(c.LastModified)}.`} actions={<Button variant="outline" size="sm" className="bg-card" onClick={() => edit("general")} data-testid="config-edit"><PencilIcon /> Edit</Button>}>
              <dl className="grid max-w-[760px] grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
                <Row k="Description" v={c.Description || <span className="text-muted-foreground">None</span>} />
                <Row k="Memory" v={<span data-testid="cfg-memory-value">{c.MemorySize} MB</span>} />
                <Row k="Timeout" v={<span data-testid="cfg-timeout-value">{c.Timeout} seconds</span>} />
                <Row k="Ephemeral storage" v={`${c.EphemeralStorage?.Size ?? 512} MB`} />
                <Row k="Runtime" v={<span className="font-mono">{c.Runtime ?? "–"}</span>} />
                <Row k="Handler" v={<span className="font-mono">{c.Handler ?? "–"}</span>} />
                <Row k="State" v={`${c.State ?? "–"}${c.LastUpdateStatus ? `, last update ${c.LastUpdateStatus}` : ""}`} />
              </dl>
            </Card>
            <Card title="Environment variables" lede="Key-value pairs the function's process reads from its environment." actions={<Button variant="outline" size="sm" className="bg-card" onClick={() => edit("env")} data-testid="env-edit"><PencilIcon /> Edit</Button>}>
              {vars.length === 0 ? (
                <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="env-none">None.</p>
              ) : (
                <dl className="grid max-w-[760px] grid-cols-[minmax(9rem,16rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0" data-testid="env-list">
                  {vars.map(([k, v]) => <Row key={k} k={<span className="font-mono text-foreground">{k}</span>} v={<span className="font-mono">{v}</span>} />)}
                </dl>
              )}
            </Card>
            <Made className="mt-3" ops={["GetFunctionConfiguration"]} />
            <GeneralDialog key={`g${opened}`} c={c} open={editing === "general"} onOpenChange={(o) => !o && setEditing(null)} />
            <EnvDialog key={`e${opened}`} c={c} open={editing === "env"} onOpenChange={(o) => !o && setEditing(null)} />
          </div>
        );
      }}
    </Loaded>
  );
}
