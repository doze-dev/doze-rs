// The Visual policy editor of Create policy (iam__policy-create's Visual | JSON switch): the policy as permission blocks. Each block is an
// effect, a service and its actions (all of them, or the ones ticked, or ones named), and the resources (all, or ARNs). The services and
// actions are what doze-aws has built (`useLab`); any action may also be typed by name. What the blocks say is the JSON beside it.
import { PlusIcon, Trash2Icon, XIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { useLab } from "../../../api/spaces";
import { Segmented } from "../../../kit";
import { catalogue, readsOnly, type ServiceActions } from "./actions";
import { type Block, blockProblem, documentOf, newBlock } from "./visual-policy";

type Props = { initial: Block[]; onChange: (doc: string) => void };

export function VisualPolicyEditor({ initial, onChange }: Props) {
  const lab = useLab();
  const services = useMemo(() => catalogue(lab.data?.targets ?? []), [lab.data]);
  const [blocks, setBlocks] = useState<Block[]>(initial.length > 0 ? initial : [newBlock()]);
  const change = (next: Block[]) => { setBlocks(next); onChange(documentOf(next)); };
  const edit = (i: number, patch: Partial<Block>) => change(blocks.map((b, at) => (at === i ? { ...b, ...patch } : b)));
  return (
    <div className="grid gap-4" data-testid="visual-editor">
      {blocks.map((b, i) => (
        <BlockCard key={i} index={i} block={b} services={services} reading={lab.isPending} onEdit={(patch) => edit(i, patch)} onRemove={blocks.length > 1 ? () => change(blocks.filter((_, at) => at !== i)) : undefined} />
      ))}
      <div>
        <Button type="button" variant="outline" className="bg-card" onClick={() => change([...blocks, newBlock()])} data-testid="visual-add-block"><PlusIcon /> Add permissions</Button>
      </div>
    </div>
  );
}

function BlockCard({ index, block: b, services, reading, onEdit, onRemove }: { index: number; block: Block; services: ServiceActions[]; reading: boolean; onEdit: (patch: Partial<Block>) => void; onRemove?: () => void }) {
  const [typed, setTyped] = useState("");
  const [filter, setFilter] = useState("");
  const svc = services.find((s) => s.prefix === b.service);
  const problem = blockProblem(b);
  const shown = (svc?.actions ?? []).filter((a) => a.toLowerCase().includes(filter.trim().toLowerCase()));
  const named = b.actions.filter((a) => !(svc?.actions ?? []).includes(a));
  const toggle = (a: string, on: boolean) => onEdit({ actions: on ? [...b.actions, a] : b.actions.filter((x) => x !== a) });
  const addTyped = () => {
    const name = typed.trim().replace(/^[\w-]+:/, "");
    if (name === "" || b.actions.includes(name)) { setTyped(""); return; }
    onEdit({ actions: [...b.actions, name] });
    setTyped("");
  };
  return (
    <section className="grid gap-4 rounded-xl border bg-card p-5" aria-label={`Permissions ${index + 1}`} data-testid="visual-block">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 text-[15px] font-semibold text-bright">Permissions {index + 1}{problem && <Badge variant="outline" className="border-warn/40 font-normal text-warn" data-testid="visual-block-problem">{problem}</Badge>}</h3>
        <div className="flex items-center gap-2">
          <Segmented<"Allow" | "Deny"> value={b.effect} onChange={(effect) => onEdit({ effect })} label="Effect" options={[{ value: "Allow", label: "Allow" }, { value: "Deny", label: "Deny" }]} testId="visual-effect" />
          {onRemove && <Button type="button" variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={onRemove} data-testid="visual-remove"><Trash2Icon /> Remove</Button>}
        </div>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor={`svc-${index}`}>Service</Label>
        <Select value={b.service} onValueChange={(service) => onEdit({ service, actions: [], all: service === "*" })}>
          <SelectTrigger id={`svc-${index}`} className="w-full max-w-sm bg-card" data-testid="visual-service"><SelectValue placeholder={reading ? "Reading…" : "Choose a service"} /></SelectTrigger>
          <SelectContent>
            <SelectItem value="*">All services (*)</SelectItem>
            {services.map((s) => <SelectItem key={s.prefix} value={s.prefix}>{s.title} ({s.prefix})</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      {b.service !== "" && b.service !== "*" && (
        <fieldset className="grid gap-3" data-testid="visual-actions">
          <legend className="mb-1 text-[14px] font-medium text-bright">Actions allowed</legend>
          <div className="flex flex-wrap gap-2">
            <Button type="button" size="sm" variant={b.all ? "default" : "outline"} className={b.all ? "" : "bg-card"} onClick={() => onEdit({ all: !b.all })} aria-pressed={b.all} data-testid="visual-all">All {b.service} actions ({b.service}:*)</Button>
            {!b.all && <>
              <Button type="button" size="sm" variant="outline" className="bg-card" onClick={() => onEdit({ actions: [...new Set([...named, ...(svc?.actions ?? []).filter(readsOnly)])] })} data-testid="visual-read">Read only</Button>
              <Button type="button" size="sm" variant="outline" className="bg-card" onClick={() => onEdit({ actions: named })} data-testid="visual-none">Clear</Button>
            </>}
          </div>
          {!b.all && (
            <>
              <Input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter the actions" aria-label="Filter the actions" className="max-w-sm bg-card" data-testid="visual-filter" />
              <ul className="grid max-h-56 gap-1 overflow-auto rounded-lg border bg-well p-2" data-testid="visual-action-list">
                {shown.length === 0 && <li className="px-2 py-1.5 text-[13px] text-muted-foreground">{svc ? "No action matches." : "doze-aws lists no actions for this service; name them below."}</li>}
                {shown.map((a) => (
                  <li key={a}>
                    <Label className="items-center gap-2.5 rounded-md px-2 py-1 font-mono text-[13px] font-normal hover:bg-muted/60"><Checkbox checked={b.actions.includes(a)} onCheckedChange={(v) => toggle(a, v === true)} data-testid={`visual-action-${a}`} />{a}</Label>
                  </li>
                ))}
              </ul>
              <div className="flex flex-wrap items-center gap-2">
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); addTyped(); } }} placeholder="Add an action by name, such as Get*" aria-label="Add an action by name" className="max-w-sm bg-card font-mono" data-testid="visual-typed" />
                <Button type="button" variant="outline" size="sm" className="bg-card" onClick={addTyped} disabled={typed.trim() === ""} data-testid="visual-typed-add">Add</Button>
              </div>
              {named.length > 0 && (
                <ul className="flex flex-wrap gap-2" aria-label="Actions added by name">
                  {named.map((a) => (
                    <li key={a}><span className="inline-flex items-center gap-1.5 rounded-full border bg-secondary py-0.5 pr-1 pl-2.5 font-mono text-[12.5px]">{b.service}:{a}<button type="button" onClick={() => toggle(a, false)} aria-label={`Remove ${a}`} className="rounded-full p-0.5 hover:bg-muted"><XIcon className="size-3.5" /></button></span></li>
                  ))}
                </ul>
              )}
            </>
          )}
        </fieldset>
      )}
      <fieldset className="grid gap-3" data-testid="visual-resources">
        <legend className="mb-1 text-[14px] font-medium text-bright">Resources</legend>
        <Segmented<"all" | "specific"> value={b.anyResource ? "all" : "specific"} onChange={(v) => onEdit({ anyResource: v === "all" })} label="Resources" options={[{ value: "specific", label: "Specific" }, { value: "all", label: "All resources" }]} testId="visual-resource-mode" />
        {!b.anyResource && (
          <div className="grid gap-2">
            {b.resources.map((r, at) => (
              <div key={at} className="flex items-center gap-2">
                <Input value={r} onChange={(e) => onEdit({ resources: b.resources.map((x, k) => (k === at ? e.target.value : x)) })} placeholder={`arn:aws:${b.service === "*" || b.service === "" ? "sqs" : b.service}:us-east-1:000000000000:name`} aria-label={`Resource ARN ${at + 1}`} autoComplete="off" spellCheck={false} className="max-w-2xl bg-card font-mono" data-testid="visual-resource" />
                {b.resources.length > 1 && <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => onEdit({ resources: b.resources.filter((_, k) => k !== at) })}>Remove</Button>}
              </div>
            ))}
            <div><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => onEdit({ resources: [...b.resources, ""] })} data-testid="visual-add-resource"><PlusIcon /> Add ARN</Button></div>
          </div>
        )}
      </fieldset>
    </section>
  );
}
