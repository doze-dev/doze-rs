// The dialogs over a dashboard: its source (cloudwatch__dashboard-source), the widget types to add (dashboard-add-widget), and the two
// widgets that need no metric: Text (widget-text) and Alarm status (widget-alarm-status). Each changes the draft body, not the saved one.
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { JsonEditor, checkJson } from "@doze/ui/components/app/json-editor";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { cn } from "@doze/ui/lib/utils";
import { useAlarms } from "./api";
import { type Body, alarmArn, alarmWidget, parseBody, rulesBroken, textWidget, withWidget } from "./dashboard-model";
import { stateTone } from "./words";

const Words = ({ children }: { children: React.ReactNode }) => <DialogDescription className="text-[14px] leading-[22px] text-foreground">{children}</DialogDescription>;

export function SourceDialog({ text, onUpdate, onClose }: { text: string; onUpdate: (body: Body) => void; onClose: () => void }) {
  const [value, setValue] = useState(() => { const b = parseBody(text); return b ? JSON.stringify(b, null, 2) : text; });
  const check = checkJson(value, "invalid");
  const body = check.ok ? parseBody(value) : null;
  const broken = body ? rulesBroken(body) : [];
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[760px]" data-testid="source-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">View/edit source</DialogTitle><DialogDescription className="sr-only">The JSON body of the dashboard.</DialogDescription></DialogHeader>
        <JsonEditor value={value} onChange={setValue} label="Dashboard source" rows={18} empty="invalid" testId="source-editor" help={<>Each row of <span className="font-mono">metrics</span> is a namespace, a metric name, then dimension name and value pairs. Writing a row here is how you add a metric the metric picker cannot find.</>} />
        {check.ok && !body && <p className="text-[13px] text-bad" role="alert" data-testid="source-problem">A dashboard body is a JSON object with a widgets array.</p>}
        {broken.length > 0 && <Alert tone="warn" title="This breaks a rule of CloudWatch's" testId="source-rules"><ul className="list-disc pl-4">{broken.map((b) => <li key={b}>{b}</li>)}</ul></Alert>}
        <p className="text-[13px] leading-5 text-muted-foreground" data-testid="source-rules-words">Grid: a body holds 0 to 500 widgets on a 24-column grid; width is 1 to 24 and height 1 to 1000; period is a multiple of 60, default 300; view is timeSeries, singleValue, gauge, bar, pie or table. Update replaces the whole body of the page with this text, the same text PutDashboard stores; choose Save dashboard to store it. <b className="font-semibold text-foreground">Stored, not applied locally.</b></p>
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
          <Button type="button" disabled={!body} onClick={() => body && onUpdate(body)} data-testid="source-update">Update</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const TYPES = [
  ["line", "Line", "Compare metrics over periods of time"],
  ["stacked", "Stacked area", "Compare the total over time"],
  ["number", "Number", "Latest metric value with a sparkline"],
  ["gauge", "Gauge", "Metric values between a minimum and a maximum"],
  ["bar", "Bar", "Compare metrics at a point in time"],
  ["pie", "Pie", "Compare the parts of a whole"],
  ["text", "Text", "A block of text in Markdown format"],
  ["alarm", "Alarm status", "Status of up to 100 alarms in a grid"],
] as const;
export type WidgetChoice = (typeof TYPES)[number][0];

export function AddWidgetDialog({ onNext, onClose }: { onNext: (kind: WidgetChoice) => void; onClose: () => void }) {
  const [kind, setKind] = useState<WidgetChoice>("line");
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[680px]" data-testid="add-widget-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Add to this dashboard</DialogTitle><Words>Choose the type of widget to add.</Words></DialogHeader>
        <div role="radiogroup" aria-label="Widget type" className="grid gap-3 sm:grid-cols-2">
          {TYPES.map(([id, label, blurb]) => (
            <button key={id} type="button" role="radio" aria-checked={kind === id} onClick={() => setKind(id)} className={cn("grid gap-0.5 rounded-xl border bg-card p-4 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50", kind === id && "border-primary bg-pick/40")} data-testid={`widget-type-${id}`}>
              <span className="font-medium text-bright">{label}</span><span className="text-[13px] text-muted-foreground">{blurb}</span>
            </button>
          ))}
        </div>
        <p className="text-[13px] leading-5 text-muted-foreground">Line, Stacked area, Number, Gauge, Bar and Pie go on to the metric picker and the graphed metrics. Text asks for Markdown; Alarm status asks you to select the alarms.</p>
        <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button><Button type="button" onClick={() => onNext(kind)} data-testid="add-widget-next">Next</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function TextWidgetDialog({ body, onCreate, onClose }: { body: Body; onCreate: (next: Body) => void; onClose: () => void }) {
  const [markdown, setMarkdown] = useState("");
  const [clear, setClear] = useState(false);
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[600px]" data-testid="text-widget-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">New text widget</DialogTitle><DialogDescription className="sr-only">A block of Markdown text for the dashboard.</DialogDescription></DialogHeader>
        <div className="grid gap-2"><Label htmlFor="md">Markdown</Label><Textarea id="md" value={markdown} onChange={(e) => setMarkdown(e.target.value)} rows={8} className="font-mono text-[13px]" placeholder="# Runbook" data-testid="widget-markdown" /><p className="text-[13px] text-muted-foreground">Format the text with Markdown.</p></div>
        <Label className="font-normal"><Checkbox checked={clear} onCheckedChange={(v) => setClear(v === true)} data-testid="widget-transparent" /> Transparent background</Label>
        <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button><Button type="button" disabled={markdown.trim() === ""} onClick={() => onCreate(withWidget(body, textWidget(markdown, clear ? "transparent" : "solid")))} data-testid="widget-create">Create widget</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function AlarmWidgetDialog({ body, region, account, onCreate, onClose }: { body: Body; region: string; account: string; onCreate: (next: Body) => void; onClose: () => void }) {
  const alarms = useAlarms();
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const list = alarms.data ?? [];
  const toggle = (n: string) => setChosen((s) => { const x = new Set(s); if (x.has(n)) x.delete(n); else x.add(n); return x; });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[600px]" data-testid="alarm-widget-dialog">
        <DialogHeader><DialogTitle className="text-[21px] font-semibold">Alarm status</DialogTitle><Words>Tick the alarms to show in the widget.</Words></DialogHeader>
        {alarms.isPending ? null : list.length === 0 ? <Alert tone="info" title="There is no alarm to show" testId="no-alarms">Create an alarm first, then come back to add it.</Alert> : (
          <ul className="grid max-h-80 gap-1 overflow-auto" aria-label="Alarms">
            {list.map((a) => (
              <li key={a.AlarmName}><Label className="flex cursor-pointer items-center justify-between gap-3 rounded-lg px-2 py-2 font-normal hover:bg-accent"><span className="flex items-center gap-3"><Checkbox checked={chosen.has(a.AlarmName)} onCheckedChange={() => toggle(a.AlarmName)} data-testid={`alarm-${a.AlarmName}`} /><span className="font-mono text-[13px]">{a.AlarmName}</span></span><StatusDot tone={stateTone(a.StateValue)}>{a.StateValue}</StatusDot></Label></li>
            ))}
          </ul>
        )}
        <p className="text-[13px] leading-5 text-muted-foreground">The widget shows each alarm's name and state, without graphs. One alarm status widget can hold up to 100 alarms.</p>
        <DialogFooter><Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button><Button type="button" disabled={chosen.size === 0 || chosen.size > 100} onClick={() => onCreate(withWidget(body, alarmWidget([...chosen].map((n) => alarmArn(region, account, n)))))} data-testid="widget-create">Create widget</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

