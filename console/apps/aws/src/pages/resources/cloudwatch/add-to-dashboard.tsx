// The Add to dashboard dialog of the graph (cloudwatch__metrics-add-to-dashboard): the graphed rows become one widget of a dashboard.
// PutDashboard replaces the whole body, so the body is read just now, the widget added, and all of it written back.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Link } from "@tanstack/react-router";
import { RadioCards, callKindOp } from "../../../kit";
import { type DashboardAnswer, DASHBOARDS, useDashboards, usePutDashboard } from "./api";
import { parseBody, stringify, withWidget } from "./dashboard-model";
import { type GM, metricWidget } from "./graph-model";

export type WidgetKind = "line" | "stacked" | "number";

export function AddToDashboard({ rows, region, onClose }: { rows: GM[]; region: string; onClose: () => void }) {
  const navigate = useNavigate();
  const dashboards = useDashboards();
  const put = usePutDashboard();
  const [name, setName] = useState<string | null>(null);
  const [kind, setKind] = useState<WidgetKind>("line");
  const [failure, setFailure] = useState<unknown>(null);
  const names = (dashboards.data ?? []).map((x) => x.DashboardName).sort();
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
              <RadioCards<WidgetKind> name="widget" label="Widget type" value={kind} onChange={setKind} className="sm:grid-cols-3" options={[{ value: "line", title: "Line" }, { value: "stacked", title: "Stacked area" }, { value: "number", title: "Number" }]} />
            </div>
            <p className="text-[13.5px] leading-5 text-foreground" data-testid="add-words">
              The {rows.length === 1 ? "graphed metric becomes" : `${rows.length} graphed metrics become`} one widget at the next free position of <b className="font-mono font-semibold text-bright">{chosen}</b> (a widget with no x and y takes the next available position). PutDashboard replaces the entire contents of a dashboard, so adding a widget writes the existing widgets back with the new one.
            </p>
            <p className="text-[13px] text-muted-foreground"><b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the dashboard body exactly as written and never interprets it (<span className="font-mono">crates/aws-cloudwatch/src/docs.rs</span>); this console draws it.</p>
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
