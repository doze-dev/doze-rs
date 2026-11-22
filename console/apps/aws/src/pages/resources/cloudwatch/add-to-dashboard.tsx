// The Add to dashboard dialog of the graph (cloudwatch__metrics-add-to-dashboard): the graphed rows become one widget of a dashboard.
// PutDashboard replaces the whole body, so the body is read just now, the widget added, and all of it written back.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Link } from "@tanstack/react-router";
import { Label } from "@doze/ui/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@doze/ui/components/ui/radio-group";
import { callKindOp } from "../../../kit";
import { type DashboardAnswer, DASHBOARDS, useDashboards, usePutDashboard } from "./api";
import { parseBody, stringify, withWidget } from "./dashboard-model";
import { type GM, metricWidget } from "./graph-model";
import { newestSavedFirst } from "./model";

export type WidgetKind = "line" | "stacked" | "number";

export function AddToDashboard({ rows, region, onClose }: { rows: GM[]; region: string; onClose: () => void }) {
  const navigate = useNavigate();
  const dashboards = useDashboards();
  const put = usePutDashboard();
  const [name, setName] = useState<string | null>(null);
  const [kind, setKind] = useState<WidgetKind>("line");
  const [failure, setFailure] = useState<unknown>(null);
  // The one saved last comes first, as the board has it (the list is sorted the same way), so it is the one chosen to begin with.
  const names = newestSavedFirst(dashboards.data ?? []).map((x) => x.DashboardName);
  const chosen = name ?? names[0] ?? null;
  const add = async () => {
    if (!chosen) return;
    setFailure(null);
    try {
      const current = await callKindOp<DashboardAnswer>(DASHBOARDS, "GetDashboard", { DashboardName: chosen });
      const body = parseBody(current.DashboardBody);
      if (!body) throw new Error(`The body of ${chosen} is not a JSON object, so no widget can be added to it.`);
      const widget = metricWidget(rows, { view: kind === "number" ? "singleValue" : "timeSeries", stacked: kind === "stacked", region });
      await put.mutateAsync({ name: chosen, body: stringify(withWidget(body, widget)) });
      void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "dashboards", name: chosen }, search: { done: "widget" } });
    } catch (e) {
      setFailure(e);
    }
  };
  const failed = failure ? errorWords(failure) : null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[600px]" data-testid="add-to-dashboard-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Add to dashboard</DialogTitle>
          <DialogDescription className="sr-only">Add the graphed metrics to a dashboard as a widget.</DialogDescription>
        </DialogHeader>
        {dashboards.isPending ? null : names.length === 0 ? (
          <Alert tone="info" title="There is no dashboard to add to" testId="no-dashboards">Create a dashboard first: <Link to="/resources/$service/$second" params={{ service: "cloudwatch", second: "dashboards" }} search={{ dialog: "create" }} className="text-link hover:underline">Create dashboard</Link>.</Alert>
        ) : (
          <div className="grid gap-5">
            <div className="grid gap-2">
              <span className="text-[14px] font-medium text-bright">Dashboard</span>
              <Select value={chosen ?? ""} onValueChange={setName}><SelectTrigger className="w-full bg-card font-mono" aria-label="Dashboard" data-testid="dashboard-select"><SelectValue /></SelectTrigger><SelectContent>{names.map((n) => <SelectItem key={n} value={n} className="font-mono">{n}</SelectItem>)}</SelectContent></Select>
            </div>
            <div className="grid gap-2">
              <span className="text-[14px] font-medium text-bright">Widget type</span>
              <RadioGroup value={kind} onValueChange={(v) => setKind(v as WidgetKind)} className="grid gap-3" aria-label="Widget type">
                {([["line", "Line"], ["stacked", "Stacked area"], ["number", "Number"]] as const).map(([v, label]) => (
                  <Label key={v} htmlFor={`widget-${v}`} className="flex cursor-pointer items-center gap-3 font-normal"><RadioGroupItem id={`widget-${v}`} value={v} data-testid={`widget-${v}`} />{label}</Label>
                ))}
              </RadioGroup>
            </div>
            <p className="text-[13.5px] leading-5 text-foreground" data-testid="add-words">
              The {rows.length === 1 ? "graphed metric becomes" : `${rows.length} graphed metrics become`} one widget at the next free position of <b className="font-mono font-semibold text-bright">{chosen}</b> (a widget with no x and y takes the next available position). PutDashboard replaces the entire contents of a dashboard, so adding a widget writes the existing widgets back with the new one.
            </p>
            <p className="text-[13px] text-muted-foreground"><b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the dashboard body exactly as written and never interprets it; this console draws it.</p>
          </div>
        )}
        {failed && <Alert tone="bad" title={`The widget was not added${failed.code ? `: ${failed.code}` : ""}`} testId="add-refused">{failed.message}</Alert>}
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
          <Button type="button" disabled={!chosen || put.isPending} onClick={() => void add()} data-testid="add-to-dashboard-submit">{put.isPending ? "Adding…" : "Add to dashboard"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
